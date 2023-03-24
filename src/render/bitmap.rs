pub use rect_packer::Rect;

use {
    crate::res,
    anyhow::Context,
    bytemuck::{bytes_of, Pod, Zeroable},
    pak::Pak,
    rect_packer::{Config, Packer},
    screen_13::prelude::*,
    std::{fmt, sync::Arc},
};

// TODO: PRs for rect_packer: Debug impl and can_pack should take u32 not i32 (same for rect w/h)

struct Atlas {
    packer: Packer,
    image: Arc<Image>,
}

impl fmt::Debug for Atlas {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Atlas").field("image", &self.image).finish()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Bitmap(usize, Rect, bool);

impl Bitmap {
    pub fn size(self) -> (u32, u32) {
        (
            self.1.width.try_into().unwrap_or_default(),
            self.1.height.try_into().unwrap_or_default(),
        )
    }
}

#[derive(Debug)]
pub struct BitmapBuffer {
    atlases: Vec<Atlas>,
    bitmap_pipeline: Arc<GraphicPipeline>,
    device: Arc<Device>,
    pending_bitmaps: Vec<(Bitmap, Arc<Image>)>,
    pool: LazyPool,

    temp_atlas_nodes: Vec<ImageNode>,
    temp_alpha_images: Vec<(u32, Rect, Rect)>,
}

impl BitmapBuffer {
    const PENDING_BITMAP_BATCH_SIZE: usize = 16;
    const IMAGE_SUBRESOURCE_LAYERS: vk::ImageSubresourceLayers = vk::ImageSubresourceLayers {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        mip_level: 0,
        base_array_layer: 0,
        layer_count: 1,
    };

    pub fn new(device: &Arc<Device>) -> anyhow::Result<Self> {
        let device = Arc::clone(device);
        let pool = LazyPool::new(&device);

        let mut res_pak = res::open_pak().context("Opening pak")?;
        let bitmap_pipeline = Arc::new(
            GraphicPipeline::create(
                &device,
                GraphicPipelineInfo::new()
                    .blend(BlendMode::ALPHA)
                    .cull_mode(vk::CullModeFlags::NONE),
                [
                    Shader::new_vertex(
                        res_pak
                            .read_blob(res::SHADER_BITMAP_VERT_SPIRV)
                            .context("Reading vert shader")?
                            .as_slice(),
                    ),
                    Shader::new_fragment(
                        res_pak
                            .read_blob(res::SHADER_BITMAP_FRAG_SPIRV)
                            .context("Reading frag shader")?
                            .as_slice(),
                    ),
                ],
            )
            .context("Creating pipeline")?,
        );

        Ok(Self {
            atlases: Default::default(),
            bitmap_pipeline,
            device,
            pending_bitmaps: Default::default(),
            pool,
            temp_atlas_nodes: Default::default(),
            temp_alpha_images: Default::default(),
        })
    }

    pub fn load_bitmap(
        &mut self,
        queue_index: usize,
        image: Arc<Image>,
        has_alpha: bool,
    ) -> Result<Bitmap, DriverError> {
        let mut atlas_idx = self
            .atlases
            .iter()
            .enumerate()
            .find(|(_, atlas)| {
                atlas
                    .packer
                    .can_pack(image.info.width as _, image.info.height as _, false)
            })
            .map(|(atlas_idx, _)| atlas_idx);

        if atlas_idx.is_none() {
            let image = Arc::new(Image::create(
                &self.device,
                ImageInfo::new_2d(
                    vk::Format::R8G8B8A8_UNORM,
                    2048,
                    2048,
                    vk::ImageUsageFlags::SAMPLED
                        | vk::ImageUsageFlags::TRANSFER_DST
                        | vk::ImageUsageFlags::TRANSFER_SRC,
                ),
            )?);

            let mut render_graph = RenderGraph::new();
            let image_node = render_graph.bind_node(&image);
            render_graph.clear_color_image(image_node);
            render_graph
                .resolve()
                .submit(&mut self.pool, 0, queue_index)?;

            atlas_idx = Some(self.atlases.len());
            self.atlases.push(Atlas {
                packer: Packer::new(Config {
                    width: 2046,
                    height: 2046,
                    border_padding: 0,
                    rectangle_padding: 1,
                }),
                image,
            });
        }

        let atlas_idx = atlas_idx.unwrap_or_default();
        let mut rect = self.atlases[atlas_idx]
            .packer
            .pack(image.info.width as _, image.info.height as _, false)
            .unwrap();
        rect.x += 1;
        rect.y += 1;

        let bitmap = Bitmap(atlas_idx, rect, has_alpha);
        self.pending_bitmaps.push((bitmap, image));

        if self.pending_bitmaps.len() >= Self::PENDING_BITMAP_BATCH_SIZE {
            self.record_pending_bitmaps(queue_index)?;
        }

        Ok(bitmap)
    }

    pub fn record<'a>(
        &mut self,
        render_graph: &mut RenderGraph,
        framebuffer_image: impl Into<AnyImageNode>,
        bitmaps: impl IntoIterator<Item = &'a (Bitmap, Rect)>,
    ) -> Result<(), DriverError> {
        let framebuffer_image = framebuffer_image.into();
        let framebuffer_info = render_graph.node_info(framebuffer_image);

        self.record_pending_bitmaps(0)?;

        self.temp_atlas_nodes.clear();

        for atlas in &self.atlases {
            self.temp_atlas_nodes
                .push(render_graph.bind_node(&atlas.image));
        }

        for (Bitmap(atlas_idx, atlas_rect, has_alpha), bitmap_rect) in bitmaps.into_iter().copied()
        {
            let atlas_image = self.temp_atlas_nodes[atlas_idx];

            if has_alpha
                || bitmap_rect.x < 0
                || bitmap_rect.y < 0
                || bitmap_rect.x + bitmap_rect.width < 0
                || bitmap_rect.y + bitmap_rect.height < 0
                || bitmap_rect.x >= framebuffer_info.width as i32
                || bitmap_rect.y >= framebuffer_info.height as i32
                || bitmap_rect.x + bitmap_rect.width >= framebuffer_info.width as i32
                || bitmap_rect.y + bitmap_rect.height >= framebuffer_info.height as i32
            {
                self.temp_alpha_images
                    .push((atlas_idx as _, atlas_rect, bitmap_rect));
            } else if atlas_rect.width == bitmap_rect.width
                && atlas_rect.height == bitmap_rect.height
            {
                render_graph.copy_image_region(
                    atlas_image,
                    framebuffer_image,
                    &vk::ImageCopy {
                        src_subresource: Self::IMAGE_SUBRESOURCE_LAYERS,
                        src_offset: vk::Offset3D {
                            x: atlas_rect.x,
                            y: atlas_rect.y,
                            z: 0,
                        },
                        dst_subresource: Self::IMAGE_SUBRESOURCE_LAYERS,
                        dst_offset: vk::Offset3D {
                            x: bitmap_rect.x,
                            y: bitmap_rect.y,
                            z: 0,
                        },
                        extent: vk::Extent3D {
                            width: bitmap_rect.width as _,
                            height: bitmap_rect.height as _,
                            depth: 1,
                        },
                    },
                );
            } else {
                render_graph.blit_image_region(
                    atlas_image,
                    framebuffer_image,
                    vk::Filter::NEAREST,
                    &vk::ImageBlit {
                        src_subresource: Self::IMAGE_SUBRESOURCE_LAYERS,
                        src_offsets: [
                            vk::Offset3D {
                                x: atlas_rect.x,
                                y: atlas_rect.y,
                                z: 0,
                            },
                            vk::Offset3D {
                                x: atlas_rect.x + atlas_rect.width,
                                y: atlas_rect.y + atlas_rect.height,
                                z: 1,
                            },
                        ],
                        dst_subresource: Self::IMAGE_SUBRESOURCE_LAYERS,
                        dst_offsets: [
                            vk::Offset3D {
                                x: bitmap_rect.x,
                                y: bitmap_rect.y,
                                z: 0,
                            },
                            vk::Offset3D {
                                x: bitmap_rect.x + bitmap_rect.width,
                                y: bitmap_rect.y + bitmap_rect.height,
                                z: 1,
                            },
                        ],
                    },
                );
            }
        }

        if !self.temp_alpha_images.is_empty() {
            let framebuffer_info = render_graph.node_info(framebuffer_image);
            let mut pass = render_graph
                .begin_pass("Bitmaps")
                .bind_pipeline(&self.bitmap_pipeline)
                .load_color(0, framebuffer_image)
                .store_color(0, framebuffer_image);

            for atlas_idx in 0..self.atlases.len() {
                pass =
                    pass.read_descriptor((0, [atlas_idx as u32]), self.temp_atlas_nodes[atlas_idx]);
            }

            let alpha_images = self.temp_alpha_images.drain(..).collect::<Box<[_]>>();

            pass.record_subpass(move |subpass, _| {
                for (atlas_idx, atlas_rect, bitmap_rect) in alpha_images.iter().copied() {
                    subpass
                        .push_constants(bytes_of(&BitmapPushConstants {
                            src: [
                                atlas_rect.x as _,
                                atlas_rect.y as _,
                                atlas_rect.width as _,
                                atlas_rect.height as _,
                            ],
                            dst: [
                                bitmap_rect.x as _,
                                bitmap_rect.y as _,
                                bitmap_rect.width as _,
                                bitmap_rect.height as _,
                            ],
                            color_size: [framebuffer_info.width, framebuffer_info.height],
                            atlas_idx,
                        }))
                        .draw(6, 1, 0, 0);
                }
            });
        }

        Ok(())
    }

    fn record_pending_bitmaps(&mut self, queue_index: usize) -> Result<(), DriverError> {
        if self.pending_bitmaps.is_empty() {
            return Ok(());
        }

        let mut render_graph = RenderGraph::new();

        self.temp_atlas_nodes.clear();
        for atlas in &self.atlases {
            self.temp_atlas_nodes
                .push(render_graph.bind_node(&atlas.image));
        }

        for (Bitmap(atlas_idx, rect, _), image) in self.pending_bitmaps.drain(..) {
            let atlas_node = self.temp_atlas_nodes[atlas_idx];
            let image_node = render_graph.bind_node(image);

            render_graph.copy_image_region(
                image_node,
                atlas_node,
                &vk::ImageCopy {
                    src_subresource: Self::IMAGE_SUBRESOURCE_LAYERS,
                    src_offset: vk::Offset3D { x: 0, y: 0, z: 0 },
                    dst_subresource: Self::IMAGE_SUBRESOURCE_LAYERS,
                    dst_offset: vk::Offset3D {
                        x: rect.x,
                        y: rect.y,
                        z: 0,
                    },
                    extent: vk::Extent3D {
                        width: rect.width as _,
                        height: rect.height as _,
                        depth: 1,
                    },
                },
            );
        }

        render_graph
            .resolve()
            .submit(&mut self.pool, 0, queue_index)?;

        Ok(())
    }
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct BitmapPushConstants {
    src: [u32; 4],
    dst: [u32; 4],
    color_size: [u32; 2],
    atlas_idx: u32,
}
