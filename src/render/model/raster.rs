use {
    super::{
        super::{
            bounding_sphere::BoundingSpherePipeline, camera::Camera,
            excl_sum::ExclusiveSumPipeline, lease_storage_buffer, lease_uniform_buffer,
        },
        Geometry, Mesh, MeshFlags, Model, ModelBufferInfo, ModelInstanceData, Technique,
        MAX_MATERIALS_PER_MODEL,
    },
    crate::res,
    anyhow::Context,
    bytemuck::{bytes_of, cast_slice, Pod, Zeroable},
    glam::{Mat4, Quat, Vec3},
    screen_13::prelude::*,
    std::{
        cell::RefCell,
        iter::repeat,
        mem::size_of,
        ops::{Index, IndexMut},
        sync::Arc,
    },
};

#[cfg(not(feature = "hot-shaders"))]
use super::super::{open_res_pak, read_blob};

#[cfg(feature = "hot-shaders")]
use {super::super::res_shader_dir, screen_13_hot::prelude::*};

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct BoundingSphere {
    center: Vec3,
    radius: f32,
}

impl BoundingSphere {
    const SIZE: vk::DeviceSize = size_of::<Self>() as vk::DeviceSize;
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct MeshInstanceRef {
    mesh_idx: u32,
    model_instance_idx: u32,
}

impl MeshInstanceRef {
    const SIZE: vk::DeviceSize = size_of::<Self>() as vk::DeviceSize;
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct ModelInstanceRef {
    material_indices: [u32; MAX_MATERIALS_PER_MODEL],
    rotation: Quat,
    translation: Vec3,
    model_idx: u32,
}

impl ModelInstanceRef {
    const SIZE: vk::DeviceSize = size_of::<Self>() as vk::DeviceSize;
}

#[cfg(not(feature = "hot-shaders"))]
#[derive(Debug)]
struct Pipelines {
    bounding_sphere: BoundingSpherePipeline,
    excl_sum: ExclusiveSumPipeline,
    mesh_cmd: Arc<ComputePipeline>,
    mesh_cull: Arc<ComputePipeline>,
    mesh_draw: Arc<GraphicPipeline>,
    subgroup_size: u32,
}

#[cfg(feature = "hot-shaders")]
#[derive(Debug)]
struct Pipelines {
    bounding_sphere: BoundingSpherePipeline,
    excl_sum: ExclusiveSumPipeline,
    mesh_cmd: HotComputePipeline,
    mesh_cull: HotComputePipeline,
    mesh_draw: HotGraphicPipeline,
    subgroup_size: u32,
}

impl Pipelines {
    #[cfg(not(feature = "hot-shaders"))]
    fn new(device: &Arc<Device>) -> anyhow::Result<Self> {
        let Vulkan11Properties { subgroup_size, .. } = device.physical_device.properties_v1_1;
        let mut res_pak = open_res_pak()?;

        let bounding_sphere = BoundingSpherePipeline::new(device, &mut res_pak)
            .context("Creating bounding sphere pipeline")?;
        let excl_sum = ExclusiveSumPipeline::new(device, &mut res_pak)
            .context("Creating exclusive sum pipelines")?;

        let mesh_cmd = Arc::new(
            ComputePipeline::create(
                &device,
                ComputePipelineInfo::default(),
                Shader::new_compute(
                    read_blob(&mut res_pak, res::SHADER_MODEL_RASTER_MESH_CMD_COMP_SPIRV)?
                        .as_slice(),
                )
                .specialization_info(Self::subgroup_specialization_info(subgroup_size)),
            )
            .context("Creating mesh command pipeline")?,
        );

        let mesh_cull = Arc::new(
            ComputePipeline::create(
                &device,
                ComputePipelineInfo::default(),
                Shader::new_compute(
                    read_blob(&mut res_pak, res::SHADER_MODEL_RASTER_MESH_CULL_COMP_SPIRV)?
                        .as_slice(),
                )
                .specialization_info(Self::subgroup_specialization_info(subgroup_size)),
            )
            .context("Creating mesh cull pipeline")?,
        );

        let mesh_draw = Arc::new(
            GraphicPipeline::create(
                device,
                GraphicPipelineInfo::new(),
                [
                    Shader::new_vertex(read_blob(
                        &mut res_pak,
                        res::SHADER_MODEL_RASTER_MESH_DRAW_VERT_SPIRV,
                    )?),
                    Shader::new_fragment(read_blob(
                        &mut res_pak,
                        res::SHADER_MODEL_RASTER_MESH_DRAW_FRAG_SPIRV,
                    )?),
                ],
            )
            .context("Creating mesh draw pipeline")?,
        );

        Ok(Self {
            bounding_sphere,
            excl_sum,
            mesh_cmd,
            mesh_cull,
            mesh_draw,
            subgroup_size,
        })
    }

    #[cfg(feature = "hot-shaders")]
    fn new(device: &Arc<Device>) -> anyhow::Result<Self> {
        let PhysicalDeviceVulkan11Properties { subgroup_size, .. } = device.vulkan_1_1_properties;
        let shader_dir = res_shader_dir();

        let bounding_sphere =
            BoundingSpherePipeline::new(device).context("Creating bounding sphere pipeline")?;
        let excl_sum =
            ExclusiveSumPipeline::new(device).context("Creating exclusive sum pipelines")?;

        let mesh_cmd = HotComputePipeline::create(
            &device,
            ComputePipelineInfo::default(),
            HotShader::new_compute(shader_dir.join("model/raster/mesh_cull.comp"))
                .specialization_info(Self::subgroup_specialization_info(subgroup_size)),
        )
        .context("Creating hot mesh command pipeline")?;

        let mesh_cull = HotComputePipeline::create(
            &device,
            ComputePipelineInfo::default(),
            HotShader::new_compute(shader_dir.join("model/raster/mesh_cull.comp"))
                .specialization_info(Self::subgroup_specialization_info(subgroup_size)),
        )
        .context("Creating hot mesh cull pipeline")?;

        let mesh_draw = HotGraphicPipeline::create(
            &device,
            GraphicPipelineInfo::new(),
            [
                HotShader::new_vertex(shader_dir.join("model/raster/mesh_draw.vert")),
                HotShader::new_fragment(shader_dir.join("model/raster/mesh_draw.frag")),
            ],
        )
        .context("Creating hot mesh draw pipeline")?;

        Ok(Self {
            bounding_sphere,
            excl_sum,
            mesh_cmd,
            mesh_cull,
            mesh_draw,
            subgroup_size,
        })
    }

    #[inline(always)]
    fn mesh_cmd(&mut self) -> &Arc<ComputePipeline> {
        #[cfg(not(feature = "hot-shaders"))]
        let res = &self.mesh_cmd;

        #[cfg(feature = "hot-shaders")]
        let res = self.mesh_cmd.hot();

        res
    }

    #[inline(always)]
    fn mesh_cull(&mut self) -> &Arc<ComputePipeline> {
        #[cfg(not(feature = "hot-shaders"))]
        let res = &self.mesh_cull;

        #[cfg(feature = "hot-shaders")]
        let res = self.mesh_cull.hot();

        res
    }

    #[inline(always)]
    fn mesh_draw(&mut self) -> &Arc<GraphicPipeline> {
        #[cfg(not(feature = "hot-shaders"))]
        let res = &self.mesh_draw;

        #[cfg(feature = "hot-shaders")]
        let res = self.mesh_draw.hot();

        res
    }

    fn subgroup_specialization_info(subgroup_size: u32) -> SpecializationInfo {
        SpecializationInfo {
            data: subgroup_size.to_ne_bytes().to_vec(),
            map_entries: vec![vk::SpecializationMapEntry {
                constant_id: 0,
                offset: 0,
                size: size_of::<u32>(),
            }],
        }
    }
}

#[derive(Debug)]
pub(super) struct Raster {
    bounding_sphere_buf: Arc<Buffer>,
    draw_cmd_buf: Arc<Buffer>,
    draw_count_buf: Arc<Buffer>,
    draw_instance_buf: Arc<Buffer>,

    mesh_count: u32,

    mesh_instance_buf: Arc<Buffer>,
    mesh_instance_count: u32,
    mesh_instance_dirty: usize,

    mesh_instance_count_buf: Arc<Buffer>,
    mesh_instance_count_dirty: Vec<bool>,
    mesh_instance_counts: Vec<u32>,

    model_instance_buf: Arc<Buffer>,
    model_instance_dirty: Vec<bool>,
    model_instances: Vec<ModelInstanceData>,

    model_mesh_count: Vec<u32>,

    pool: LazyPool,
    pipelines: Pipelines,
}

impl Raster {
    const INSTANCE_GRANULARITY: usize = 64;

    pub fn new(device: &Arc<Device>, info: ModelBufferInfo) -> anyhow::Result<Self> {
        let bounding_sphere_buf = Arc::new(Buffer::create(
            device,
            BufferInfo::new(
                info.mesh_capacity * BoundingSphere::SIZE,
                vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
            ),
        )?);
        let draw_cmd_buf = Arc::new(Buffer::create(
            device,
            BufferInfo::new(
                info.mesh_capacity * size_of::<vk::DrawIndexedIndirectCommand>() as vk::DeviceSize,
                vk::BufferUsageFlags::INDIRECT_BUFFER | vk::BufferUsageFlags::STORAGE_BUFFER,
            ),
        )?);
        let draw_count_buf = Arc::new(Buffer::create(
            device,
            BufferInfo::new(
                size_of::<u32>() as _,
                vk::BufferUsageFlags::INDIRECT_BUFFER | vk::BufferUsageFlags::STORAGE_BUFFER,
            ),
        )?);
        let draw_instance_buf = Arc::new(Buffer::create(
            device,
            BufferInfo::new(
                info.mesh_capacity * size_of::<u32>() as vk::DeviceSize,
                vk::BufferUsageFlags::STORAGE_BUFFER,
            ),
        )?);
        let mesh_instance_count_buf = Arc::new(Buffer::create(
            device,
            BufferInfo::new(
                info.mesh_capacity * size_of::<u32>() as vk::DeviceSize,
                vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
            ),
        )?);
        let mesh_instance_buf = Arc::new(Buffer::create(
            device,
            BufferInfo::new(
                // TODO: This should be an "instance_capacity"
                info.mesh_capacity * MeshInstanceRef::SIZE,
                vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
            ),
        )?);
        let model_instance_buf = Arc::new(Buffer::create(
            device,
            BufferInfo::new(
                // TODO: This should be an "instance_capacity"
                info.model_capacity * ModelInstanceRef::SIZE,
                vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
            ),
        )?);
        let pipelines = Pipelines::new(device)?;

        let mesh_dirty_len = (info.mesh_capacity as usize + Self::INSTANCE_GRANULARITY - 1)
            / Self::INSTANCE_GRANULARITY;
        let mesh_instance_dirty = vec![false; mesh_dirty_len];
        let mesh_instance_count_dirty = vec![false; mesh_dirty_len];

        let model_instance_dirty_len = (info.model_capacity as usize + Self::INSTANCE_GRANULARITY
            - 1)
            / Self::INSTANCE_GRANULARITY;
        let model_instance_dirty = vec![false; model_instance_dirty_len];

        let pool = LazyPool::new(device);

        Ok(Self {
            bounding_sphere_buf,
            draw_cmd_buf,
            draw_count_buf,
            draw_instance_buf,
            mesh_count: 0,
            mesh_instance_buf,
            mesh_instance_count: 0,
            mesh_instance_dirty: 0,
            mesh_instance_count_buf,
            mesh_instance_count_dirty,
            mesh_instance_counts: Default::default(),
            model_instance_buf,
            model_instance_dirty,
            model_instances: Default::default(),
            model_mesh_count: Vec::with_capacity(info.model_capacity as usize),
            pool,
            pipelines,
        })
    }

    fn update_mesh_instance_buf(
        &mut self,
        render_graph: &mut RenderGraph,
    ) -> Result<BufferNode, DriverError> {
        let mesh_instance_buf = render_graph.bind_node(&self.mesh_instance_buf);

        if self.mesh_instance_dirty < self.model_instances.len() {
            let temp_len = self.mesh_instance_count as vk::DeviceSize * MeshInstanceRef::SIZE;
            let mut temp_buf = self.pool.lease(BufferInfo::new_mappable(
                temp_len,
                vk::BufferUsageFlags::TRANSFER_SRC,
            ))?;
            let temp_data = Buffer::mapped_slice_mut(&mut temp_buf);

            let mut base = 0;
            for (model_instance_idx, model_instance) in self.model_instances.iter().enumerate() {
                let model_instance_idx = model_instance_idx as u32;

                for mesh_offset in 0..self.model_mesh_count[model_instance.model.model_idx] {
                    let start = base as usize * MeshInstanceRef::SIZE as usize;
                    let end = start + MeshInstanceRef::SIZE as usize;
                    let mesh_idx = model_instance.model.mesh_idx as u32 + mesh_offset;

                    temp_data[start..end].copy_from_slice(bytes_of(&MeshInstanceRef {
                        mesh_idx,
                        model_instance_idx,
                    }));

                    base += 1;
                }
            }

            let temp_buf = render_graph.bind_node(temp_buf);
            render_graph.copy_buffer_region(
                temp_buf,
                mesh_instance_buf,
                &vk::BufferCopy {
                    src_offset: 0,
                    dst_offset: 0,
                    size: temp_len,
                },
            );

            // let mut mesh_instance_count = 0;
            // self.model_instances[self.mesh_instance_dirty..]
            //     .iter()
            //     .for_each(|model_instance| {
            //         mesh_instance_count += self.model_mesh_count[model_instance.model.model_idx]
            //     });

            // let temp_len = mesh_instance_count as vk::DeviceSize * MeshInstanceRef::SIZE;
            // let mut temp_buf = self.pool.lease(BufferInfo::new_mappable(
            //     temp_len,
            //     vk::BufferUsageFlags::TRANSFER_SRC,
            // ))?;
            // let temp_data = Buffer::mapped_slice_mut(&mut temp_buf);

            // self.model_instances[self.mesh_instance_dirty..]
            //     .iter()
            //     .enumerate()
            //     .for_each(|(model_instance_offset, model_instance)| {
            //         let model_instance_idx =
            //             (self.mesh_instance_dirty + model_instance_offset) as u32;

            //         for mesh_offset in
            //             0..self.model_mesh_count[model_instance.model.model_idx]
            //         {
            //             let start = mesh_offset as usize * MeshInstanceRef::SIZE as usize;
            //             let end = start + MeshInstanceRef::SIZE as usize;
            //             let mesh_idx = model_instance.model.mesh_idx as u32 + mesh_offset;

            //             temp_data[start..end].copy_from_slice(bytes_of(&MeshInstanceRef {
            //                 mesh_idx,
            //                 model_instance_idx,
            //             }));
            //         }
            //     });

            // let temp_buf = render_graph.bind_node(temp_buf);
            // render_graph.copy_buffer_region(
            //     temp_buf,
            //     mesh_instance_buf,
            //     &vk::BufferCopy {
            //         src_offset: 0,
            //         dst_offset: self.mesh_instance_count as vk::DeviceSize * MeshInstanceRef::SIZE - temp_len,
            //         size: temp_len,
            //     },
            // );

            self.mesh_instance_dirty = self.model_instances.len();
        }

        Ok(mesh_instance_buf)
    }

    fn update_mesh_instance_count_buf(
        &mut self,
        render_graph: &mut RenderGraph,
    ) -> Result<BufferNode, DriverError> {
        let mesh_instance_count_buf = render_graph.bind_node(&self.mesh_instance_count_buf);

        let temp_buf_len = self.mesh_instance_counts.len() as vk::DeviceSize * 4;
        let mut temp_buf = self.pool.lease(BufferInfo::new_mappable(
            temp_buf_len,
            vk::BufferUsageFlags::TRANSFER_SRC,
        ))?;

        let temp_data = Buffer::mapped_slice_mut(&mut temp_buf);

        temp_data[0..temp_buf_len as usize].copy_from_slice(cast_slice(&self.mesh_instance_counts));

        let temp_buf = render_graph.bind_node(temp_buf);

        render_graph.copy_buffer_region(
            temp_buf,
            mesh_instance_count_buf,
            &vk::BufferCopy {
                src_offset: 0,
                dst_offset: 0,
                size: temp_buf_len,
            },
        );

        // let region_count = self
        //     .mesh_instance_count_dirty
        //     .iter()
        //     .copied()
        //     .filter(|is_dirty| *is_dirty)
        //     .count();
        // if region_count == 0 {
        //     return Ok(mesh_instance_count_buf);
        // }

        // const REGION_SIZE: vk::DeviceSize =
        //     size_of::<u32>() as vk::DeviceSize * Raster::INSTANCE_GRANULARITY as vk::DeviceSize;

        // let temp_buf_len = region_count as vk::DeviceSize * REGION_SIZE;
        // let mut temp_buf = self.pool.lease(BufferInfo::new_mappable(
        //     temp_buf_len,
        //     vk::BufferUsageFlags::TRANSFER_SRC,
        // ))?;

        // let mut regions = Vec::with_capacity(region_count);
        // let temp_data = Buffer::mapped_slice_mut(&mut temp_buf);

        // for (src_idx, dst_idx) in self
        //     .mesh_instance_count_dirty
        //     .iter()
        //     .copied()
        //     .enumerate()
        //     .filter_map(|(idx, is_dirty)| is_dirty.then(|| idx))
        //     .enumerate()
        // {
        //     let src_offset = src_idx as vk::DeviceSize * REGION_SIZE;
        //     temp_data[src_offset as usize..src_offset as usize + REGION_SIZE as usize]
        //         .copy_from_slice(cast_slice(
        //             &self.mesh_instance_counts[src_idx..src_idx + Raster::INSTANCE_GRANULARITY],
        //         ));

        //     regions.push(vk::BufferCopy {
        //         src_offset,
        //         dst_offset: dst_idx as vk::DeviceSize * REGION_SIZE,
        //         size: REGION_SIZE,
        //     });
        // }

        // let temp_buf = render_graph.bind_node(temp_buf);
        // render_graph.copy_buffer_regions(temp_buf, mesh_instance_count_buf, regions);

        // self.mesh_instance_count_dirty.fill(false);

        Ok(mesh_instance_count_buf)
    }

    fn update_model_instance_buf(
        &mut self,
        render_graph: &mut RenderGraph,
    ) -> Result<BufferNode, DriverError> {
        let model_instance_buf = render_graph.bind_node(&self.model_instance_buf);

        let temp_buf_len = self.model_instances.len() as vk::DeviceSize * ModelInstanceRef::SIZE;
        let mut temp_buf = self.pool.lease(BufferInfo::new_mappable(
            temp_buf_len,
            vk::BufferUsageFlags::TRANSFER_SRC,
        ))?;

        let temp_data = Buffer::mapped_slice_mut(&mut temp_buf);
        let model_instances = self
            .model_instances
            .iter()
            .map(|model_instance| {
                let mut material_indices = [0u32; MAX_MATERIALS_PER_MODEL];
                for (idx, material) in model_instance.materials.iter().enumerate() {
                    material_indices[idx] = material.material_index;
                }

                let ModelInstanceData {
                    rotation,
                    translation,
                    model: Model { model_idx, .. },
                    ..
                } = *model_instance;

                ModelInstanceRef {
                    material_indices,
                    rotation,
                    translation,
                    model_idx: model_idx as _,
                }
            })
            .collect::<Box<_>>();

        temp_data[0..temp_buf_len as usize].copy_from_slice(cast_slice(&model_instances));

        let temp_buf = render_graph.bind_node(temp_buf);

        render_graph.copy_buffer_region(
            temp_buf,
            model_instance_buf,
            &vk::BufferCopy {
                src_offset: 0,
                dst_offset: 0,
                size: temp_buf_len,
            },
        );

        // let region_count = self
        //     .model_instance_dirty
        //     .iter()
        //     .copied()
        //     .filter(|is_dirty| *is_dirty)
        //     .count();
        // if region_count == 0 {
        //     return Ok(model_instance_buf);
        // }

        // const REGION_SIZE: vk::DeviceSize =
        //     ModelInstanceRef::SIZE * Raster::INSTANCE_GRANULARITY as vk::DeviceSize;

        // let temp_buf_len = region_count as vk::DeviceSize * REGION_SIZE;
        // let mut temp_buf = self.pool.lease(BufferInfo::new_mappable(
        //     temp_buf_len,
        //     vk::BufferUsageFlags::TRANSFER_SRC,
        // ))?;

        // let mut regions = Vec::with_capacity(region_count);
        // let temp_data = Buffer::mapped_slice_mut(&mut temp_buf);

        // for (src_idx, dst_idx) in self
        //     .model_instance_dirty
        //     .iter()
        //     .copied()
        //     .enumerate()
        //     .filter_map(|(idx, is_dirty)| is_dirty.then(|| idx))
        //     .enumerate()
        // {
        //     let src_offset = src_idx as vk::DeviceSize * REGION_SIZE;

        //     thread_local! {
        //         static REFS: RefCell<Vec<ModelInstanceRef>> = Default::default();
        //     }

        //     REFS.with(|refs| {
        //         let mut refs = refs.borrow_mut();

        //         let end_idx = self
        //             .model_instances
        //             .len()
        //             .min(src_idx + Self::INSTANCE_GRANULARITY);

        //         refs.clear();
        //         refs.extend(self.model_instances[src_idx..end_idx].iter().map(|data| {
        //             let mut material_indices = [0u32; MAX_MATERIALS_PER_MODEL];
        //             for (idx, material) in data.materials.iter().enumerate() {
        //                 material_indices[idx] = material.material_index;
        //             }

        //             let ModelInstanceData {
        //                 rotation,
        //                 translation,
        //                 model: Model { model_idx, .. },
        //                 ..
        //             } = *data;

        //             ModelInstanceRef {
        //                 material_indices,
        //                 rotation,
        //                 translation,
        //                 model_idx: model_idx as _,
        //             }
        //         }));

        //         let src_offset = src_offset as usize;
        //         let data = cast_slice(&refs);

        //         temp_data[src_offset..src_offset + data.len()].copy_from_slice(data);
        //     });

        //     regions.push(vk::BufferCopy {
        //         src_offset,
        //         dst_offset: dst_idx as vk::DeviceSize * REGION_SIZE,
        //         size: REGION_SIZE,
        //     });
        // }

        // let temp_buf = render_graph.bind_node(temp_buf);
        // render_graph.copy_buffer_regions(temp_buf, model_instance_buf, regions);

        // self.model_instance_dirty.fill(false);

        Ok(model_instance_buf)
    }
}

impl Index<usize> for Raster {
    type Output = ModelInstanceData;

    fn index(&self, idx: usize) -> &Self::Output {
        &self.model_instances[idx]
    }
}

impl IndexMut<usize> for Raster {
    fn index_mut(&mut self, idx: usize) -> &mut Self::Output {
        self.model_instance_dirty[idx / Self::INSTANCE_GRANULARITY] = true;

        &mut self.model_instances[idx]
    }
}

impl Technique for Raster {
    fn load_model(
        &mut self,
        render_graph: &mut RenderGraph,
        geometry_buf: BufferNode,
        geometries: &[Geometry],
    ) -> Result<(), DriverError> {
        let bounding_sphere_buf = render_graph.bind_node(&self.bounding_sphere_buf);

        for (geom_idx, geom) in geometries.iter().enumerate() {
            self.pipelines.bounding_sphere.record(
                render_graph,
                &mut self.pool,
                geometry_buf,
                geom.vertex_count,
                (geom.vertex_offset / size_of::<f32>() as vk::DeviceSize) as _,
                geom.flags.vertex_stride() as _,
                bounding_sphere_buf,
                (self.mesh_count + geom_idx as u32) as vk::DeviceSize * BoundingSphere::SIZE,
            )?;
        }

        let model_idx = self.model_mesh_count.len();
        let mesh_count = geometries.len() as u32;

        self.model_mesh_count.push(mesh_count);
        self.mesh_count += mesh_count;
        self.mesh_instance_counts
            .extend(repeat(0).take(mesh_count as _));

        let mesh_instance_count_dirty_len =
            (model_idx + mesh_count as usize + Self::INSTANCE_GRANULARITY - 1)
                / Self::INSTANCE_GRANULARITY;
        while self.mesh_instance_count_dirty.len() < mesh_instance_count_dirty_len {
            self.mesh_instance_count_dirty.push(true);
        }

        Ok(())
    }

    fn push_model_instance(&mut self, model_instance: ModelInstanceData) {
        let dirty_idx = self.model_instances.len() / Self::INSTANCE_GRANULARITY;
        if dirty_idx == self.model_instance_dirty.len() {
            self.model_instance_dirty.push(true);
        } else {
            self.model_instance_dirty[dirty_idx] = true;
        }

        let mesh_count = self.model_mesh_count[model_instance.model.model_idx];

        self.model_instances.push(model_instance);
        self.mesh_instance_count += mesh_count;

        for idx in
            model_instance.model.mesh_idx..model_instance.model.mesh_idx + mesh_count as usize
        {
            self.mesh_instance_counts[idx] += 1;

            let dirty_idx = idx / Self::INSTANCE_GRANULARITY;
            self.mesh_instance_count_dirty[dirty_idx] = true;
        }
    }

    fn record(
        &mut self,
        render_graph: &mut RenderGraph,
        framebuffer: AnyImageNode,
        camera: &mut Camera,
        geometry_buf: BufferNode,
        material_buf: BufferNode,
        mesh_buf: BufferNode,
        textures: &[Arc<Image>],
    ) -> Result<(), DriverError> {
        let mesh_instance_offset_buf = {
            let mesh_count = self.pipelines.excl_sum.align_input_count(self.mesh_count);
            let mesh_instance_offset_buf =
                render_graph.bind_node(self.pool.lease(BufferInfo::new(
                    (mesh_count as usize * size_of::<u32>()) as _,
                    vk::BufferUsageFlags::STORAGE_BUFFER,
                ))?);
            let mesh_instance_count_buf = self.update_mesh_instance_count_buf(render_graph)?;

            self.pipelines.excl_sum.record(
                render_graph,
                &mut self.pool,
                mesh_instance_count_buf,
                mesh_count,
                mesh_instance_offset_buf,
            )?;

            mesh_instance_offset_buf
        };

        let draw_cmd_buf = render_graph.bind_node(&self.draw_cmd_buf);

        {
            let mesh_count = self.mesh_count;
            let workgroup_count =
                (mesh_count + self.pipelines.subgroup_size - 1) / self.pipelines.subgroup_size;

            #[derive(Clone, Copy, Pod, Zeroable)]
            #[repr(C)]
            struct PushConstants {
                mesh_count: u32,
            }

            let push_consts = PushConstants { mesh_count };

            render_graph
                .begin_pass("Mesh command")
                .bind_pipeline(self.pipelines.mesh_cmd())
                .access_descriptor(0, draw_cmd_buf, AccessType::ComputeShaderWrite)
                .access_descriptor(1, mesh_buf, AccessType::ComputeShaderReadOther)
                .access_descriptor(
                    2,
                    mesh_instance_offset_buf,
                    AccessType::ComputeShaderReadOther,
                )
                .record_compute(move |compute, _| {
                    compute
                        .push_constants(bytes_of(&push_consts))
                        .dispatch(workgroup_count, 1, 1);
                });
        }

        let bounding_sphere_buf = render_graph.bind_node(&self.bounding_sphere_buf);
        let draw_instance_buf = render_graph.bind_node(&self.draw_instance_buf);
        let model_instance_buf = self.update_model_instance_buf(render_graph)?;
        let mesh_instance_buf = self.update_mesh_instance_buf(render_graph)?;

        {
            let mesh_instance_count = self.mesh_instance_count;
            let workgroup_count = (mesh_instance_count + self.pipelines.subgroup_size - 1)
                / self.pipelines.subgroup_size;

            render_graph
                .begin_pass("Mesh cull")
                .bind_pipeline(self.pipelines.mesh_cull())
                .access_descriptor(0, draw_cmd_buf, AccessType::ComputeShaderWrite)
                .access_descriptor(1, draw_instance_buf, AccessType::ComputeShaderWrite)
                .access_descriptor(2, model_instance_buf, AccessType::ComputeShaderReadOther)
                .access_descriptor(3, mesh_instance_buf, AccessType::ComputeShaderReadOther)
                .access_descriptor(
                    4,
                    mesh_instance_offset_buf,
                    AccessType::ComputeShaderReadOther,
                )
                .access_descriptor(5, bounding_sphere_buf, AccessType::ComputeShaderReadOther)
                .record_compute(move |compute, _| {
                    compute
                        .push_constants(&mesh_instance_count.to_ne_bytes())
                        .dispatch(workgroup_count, 1, 1);
                });
        }

        {
            let framebuffer_info = render_graph.node_info(framebuffer);
            let aspect_ratio = framebuffer_info.width as f32 / framebuffer_info.height as f32;
            let view_target = Vec3::Z;
            let view = Quat::from_rotation_y(camera.yaw.to_radians())
                * Quat::from_rotation_x(camera.pitch.to_radians());
            let view = Mat4::look_at_lh(
                camera.position,
                camera.position - view.mul_vec3(view_target),
                -Vec3::Y,
            );
            let projection = Mat4::perspective_lh(camera.fov_y, aspect_ratio, 0.1, 1000.0);
            let projection_view = projection * view;
            let camera_buf =
                render_graph.bind_node(lease_uniform_buffer(&mut self.pool, projection_view)?);

            let depth_image = render_graph.bind_node(self.pool.lease(ImageInfo::new_2d(
                vk::Format::D32_SFLOAT,
                framebuffer_info.width,
                framebuffer_info.height,
                vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT
                    | vk::ImageUsageFlags::TRANSIENT_ATTACHMENT,
            ))?);

            let mesh_count = self.mesh_count;

            let mut mesh_pass = render_graph
                .begin_pass("Mesh draw")
                .bind_pipeline(self.pipelines.mesh_draw())
                .set_depth_stencil(DepthStencilMode::DEPTH_WRITE)
                .access_node(draw_cmd_buf, AccessType::IndirectBuffer)
                .access_node(geometry_buf, AccessType::IndexBuffer)
                .access_descriptor(0, camera_buf, AccessType::VertexShaderReadUniformBuffer)
                .access_descriptor(1, draw_instance_buf, AccessType::VertexShaderReadOther)
                .access_descriptor(2, geometry_buf, AccessType::VertexShaderReadOther)
                .access_descriptor(3, geometry_buf, AccessType::Nothing)
                .access_descriptor(4, geometry_buf, AccessType::Nothing)
                .access_descriptor(5, mesh_instance_buf, AccessType::VertexShaderReadOther)
                .access_descriptor(6, mesh_buf, AccessType::VertexShaderReadOther)
                .access_descriptor(7, model_instance_buf, AccessType::VertexShaderReadOther)
                .access_descriptor(8, material_buf, AccessType::FragmentShaderReadOther);

            for (idx, texture) in textures.iter().enumerate() {
                let texture = mesh_pass.bind_node(texture);
                mesh_pass = mesh_pass.read_descriptor((9, [idx as u32]), texture);
            }

            mesh_pass
                .store_color(0, framebuffer)
                .clear_depth_stencil(depth_image)
                .store_depth_stencil(depth_image)
                .record_subpass(move |subpass, _| {
                    subpass.draw_indirect(
                        draw_cmd_buf,
                        0,
                        mesh_count,
                        size_of::<vk::DrawIndirectCommand>() as _,
                    );
                });
        }

        Ok(())
    }

    fn swap_remove_model_instance(&mut self, idx: usize) {
        self.mesh_instance_dirty = self.mesh_instance_dirty.min(idx);

        // If the instance we are swapping was marked dirty we mark the new instance region as dirty
        let last_idx = (self.model_instances.len() - 1) / Self::INSTANCE_GRANULARITY;
        if self.model_instance_dirty[last_idx] {
            let swapped_idx = idx / Self::INSTANCE_GRANULARITY;
            self.model_instance_dirty[swapped_idx] = true;
        }

        let removed_model_instance = self.model_instances.swap_remove(idx);
        let removed_mesh_count = self.model_mesh_count[removed_model_instance.model.model_idx];

        self.mesh_instance_count -= removed_mesh_count;

        for idx in removed_model_instance.model.mesh_idx
            ..removed_model_instance.model.mesh_idx + removed_mesh_count as usize
        {
            self.mesh_instance_counts[idx] -= 1;
            self.mesh_instance_count_dirty[idx / Self::INSTANCE_GRANULARITY] = true;
        }
    }
}
