mod raster;
mod ray_trace;
mod sbt;

use {
    self::{super::camera::Camera, raster::Raster, ray_trace::RayTrace},
    crate::math::{align_up_u32, align_up_u64},
    anyhow::Context,
    bitflags::bitflags,
    bytemuck::{bytes_of, cast_slice, Pod, Zeroable},
    derive_builder::{Builder, UninitializedFieldError},
    glam::{Quat, Vec3},
    pak::model::{ModelBuf, Vertex},
    screen_13::prelude::*,
    serde::{Deserialize, Serialize},
    std::{
        collections::HashMap,
        fmt::Debug,
        iter::repeat,
        mem::size_of,
        ops::{Index, IndexMut},
        sync::Arc,
    },
};

const MAX_MATERIALS_PER_MODEL: usize = 8;

fn material_array(materials: &[Material]) -> [Material; MAX_MATERIALS_PER_MODEL] {
    debug_assert!(!materials.is_empty());

    #[cfg(debug_assertions)]
    if materials.len() > MAX_MATERIALS_PER_MODEL {
        warn!(
            "Ignoring {} provided materials",
            materials.len() - MAX_MATERIALS_PER_MODEL
        );
    }

    let mut materials_array = [materials[0]; MAX_MATERIALS_PER_MODEL];
    materials_array.copy_from_slice(
        &materials
            .iter()
            .copied()
            .chain(repeat(materials[0]))
            .take(MAX_MATERIALS_PER_MODEL)
            .collect::<Box<_>>(),
    );

    materials_array
}

struct Geometry {
    flags: MeshFlags,
    index_count: u32,
    index_offset: vk::DeviceSize,
    vertex_count: u32,
    vertex_offset: vk::DeviceSize,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct Material {
    material_index: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Pod, Zeroable)]
#[repr(C)]
struct MaterialData {
    color_index: u32,
    flags: MaterialFlags,
    _0: [u8; 3],
}

impl MaterialData {
    const SIZE: vk::DeviceSize = size_of::<Self>() as _;
}

bitflags! {
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Pod, Zeroable)]
    #[repr(transparent)]
    pub struct MaterialFlags: u8 {
        const EMISSIVE = 0b0000_0001;
    }
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct Mesh {
    index_count: u32,
    index_offset: u32,
    vertex_offset: u32,
    material: u8,
    flags: MeshFlags,
    vertex_stride: u8,
    _0: u8,
}

impl Mesh {
    const SIZE: vk::DeviceSize = size_of::<Self>() as _;
}

bitflags! {
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Pod, Zeroable)]
    #[repr(transparent)]
    pub struct MeshFlags: u8 {
        const INDEX_TYPE_UINT32 = 0b0000_0001;
        const JOINTS_WEIGHTS = 0b0000_0010;
    }
}

impl MeshFlags {
    fn index_ty(self) -> vk::IndexType {
        if self.contains(Self::INDEX_TYPE_UINT32) {
            vk::IndexType::UINT32
        } else {
            vk::IndexType::UINT16
        }
    }

    fn vertex_stride(self) -> vk::DeviceSize {
        if self.contains(Self::JOINTS_WEIGHTS) {
            56
        } else {
            48
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Model {
    mesh_idx: usize,
    model_idx: usize,
}

#[derive(Debug)]
pub struct ModelBuffer {
    geometry_buf: Arc<Buffer>,
    geometry_len: vk::DeviceSize,
    material_buf: Arc<Buffer>,
    material_count: usize,
    mesh_buf: Arc<Buffer>,
    mesh_count: usize,
    model_count: usize,
    model_instance_id: usize,
    model_instance_index: HashMap<ModelInstance, usize>,
    model_instances: Vec<ModelInstance>,
    pool: LazyPool,
    textures: Vec<Arc<Image>>,
    technique: Box<dyn Technique>,
}

impl ModelBuffer {
    pub fn new(device: &Arc<Device>, info: impl Into<ModelBufferInfo>) -> anyhow::Result<Self> {
        let info: ModelBufferInfo = info.into();

        if let Some(technique) = info.technique {
            info!(
                "Using {} technique",
                if technique == ModelBufferTechnique::RayTrace {
                    "ray trace"
                } else {
                    "raster"
                }
            );
        }

        let technique = info.technique.unwrap_or_else(|| {
            if device.physical_device.ray_trace_properties.is_some() {
                info!("Defaulting to ray trace technique");

                ModelBufferTechnique::RayTrace
            } else {
                info!("Using raster technique");

                ModelBufferTechnique::Raster
            }
        });
        let geometry_usage = vk::BufferUsageFlags::STORAGE_BUFFER
            | match technique {
                ModelBufferTechnique::Raster => vk::BufferUsageFlags::empty(),
                ModelBufferTechnique::RayTrace => {
                    vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR
                        | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                }
            };
        let geometry_buf = Arc::new(Buffer::create(
            device,
            BufferInfo::new(
                info.geometry_capacity,
                vk::BufferUsageFlags::INDEX_BUFFER
                    | vk::BufferUsageFlags::VERTEX_BUFFER
                    | vk::BufferUsageFlags::TRANSFER_DST
                    | geometry_usage,
            ),
        )?);
        let material_buf = Arc::new(Buffer::create(
            device,
            BufferInfo::new(
                MaterialData::SIZE * info.material_capacity,
                vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
            ),
        )?);
        let mesh_buf = Arc::new(Buffer::create(
            device,
            BufferInfo::new(
                Mesh::SIZE * info.mesh_capacity,
                vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
            ),
        )?);

        let technique: Box<dyn Technique> = match technique {
            ModelBufferTechnique::Raster => {
                Box::new(Raster::new(device, info).context("Creating raster technique")?)
            }
            ModelBufferTechnique::RayTrace => {
                Box::new(RayTrace::new(device, info).context("Creating ray trace technique")?)
            }
        };

        let pool = LazyPool::new(device);

        Ok(Self {
            geometry_buf,
            geometry_len: 0,
            material_buf,
            material_count: 0,
            mesh_buf,
            mesh_count: 0,
            model_count: 0,
            model_instance_id: 0,
            model_instance_index: Default::default(),
            model_instances: Default::default(),
            pool,
            textures: Default::default(),
            technique,
        })
    }

    pub fn insert_model_instance(
        &mut self,
        model: Model,
        materials: &[Material],
        translation: Vec3,
        rotation: Quat,
    ) -> ModelInstance {
        let materials = material_array(materials);

        let model_instance = ModelInstance(self.model_instance_id);
        self.model_instance_id += 1;

        let index = self.model_instance_index.len();
        self.model_instance_index.insert(model_instance, index);
        self.model_instances.push(model_instance);

        debug_assert_eq!(self.model_instance_index.len(), self.model_instances.len());

        self.technique.push_model_instance(ModelInstanceData {
            materials,
            model,
            rotation,
            translation,
        });

        model_instance
    }

    pub fn load_material(
        &mut self,
        queue_index: usize,
        color: Arc<Image>,
        normal: Arc<Image>,
        params: Arc<Image>,
        emissive: Option<Arc<Image>>,
    ) -> Result<Material, DriverError> {
        let mut flags = MaterialFlags::empty();
        flags.set(MaterialFlags::EMISSIVE, emissive.is_some());

        let material_data = MaterialData {
            color_index: self.textures.len() as _,
            flags,
            _0: Default::default(),
        };

        self.textures.push(color);
        self.textures.push(normal);
        self.textures.push(params);

        if let Some(emissive) = emissive {
            self.textures.push(emissive);
        }

        let mut render_graph = RenderGraph::new();

        let temp_buf = {
            let mut buf = self.pool.lease(BufferInfo::new_mappable(
                MaterialData::SIZE,
                vk::BufferUsageFlags::TRANSFER_SRC,
            ))?;

            Buffer::copy_from_slice(&mut buf, 0, bytes_of(&material_data));

            render_graph.bind_node(buf)
        };

        let material_buf = render_graph.bind_node(&self.material_buf);

        render_graph.copy_buffer_region(
            temp_buf,
            material_buf,
            vk::BufferCopy {
                src_offset: 0,
                dst_offset: MaterialData::SIZE * self.material_count as vk::DeviceSize,
                size: MaterialData::SIZE,
            },
        );

        render_graph
            .resolve()
            .submit(&mut self.pool, 0, queue_index)?;

        let material = Material {
            material_index: self.material_count as _,
        };
        self.material_count += 1;

        Ok(material)
    }

    pub fn load_model(
        &mut self,
        queue_index: usize,
        model_buf: ModelBuf,
    ) -> Result<Model, DriverError> {
        let mesh_parts = model_buf
            .meshes()
            .iter()
            .map(|mesh| mesh.parts())
            .flatten()
            .collect::<Box<_>>();

        let model = Model {
            mesh_idx: self.mesh_count,
            model_idx: self.model_count,
        };

        let mut render_graph = RenderGraph::new();
        let geometry_buf = render_graph.bind_node(&self.geometry_buf);
        let mesh_buf = render_graph.bind_node(&self.mesh_buf);

        let mut geometries = Vec::with_capacity(mesh_parts.len());

        for mesh_part in mesh_parts.iter().copied() {
            let lods = mesh_part.lods();

            debug_assert!(!lods.is_empty());
            debug_assert!(self.geometry_len % size_of::<u32>() as vk::DeviceSize == 0);

            let base_lod = &lods[0];
            let index_buf = base_lod.as_u32();
            let index_count = index_buf.len() as u32;

            debug_assert!(index_count % 3 == 0);

            let vertex_buf = mesh_part.vertex_data();
            let vertex_ty = mesh_part.vertex();

            // All the meshes used by this program are formatted like this with an optional skin
            debug_assert!(vertex_ty.contains(Vertex::POSITION));
            debug_assert!(vertex_ty.contains(Vertex::NORMAL));
            debug_assert!(vertex_ty.contains(Vertex::TANGENT));
            debug_assert!(vertex_ty.contains(Vertex::TEXTURE0));
            debug_assert!(!vertex_ty.contains(Vertex::TEXTURE1));

            let vertex_len = vertex_buf.len() as u32;
            let vertex_stride = vertex_ty.stride() as u32;
            let vertex_count = vertex_len / vertex_stride;

            debug_assert!(vertex_len % size_of::<u32>() as u32 == 0);

            let index_is_u32 = vertex_count > u16::MAX as _;
            let index_shift = (index_is_u32 as usize + 1) as vk::DeviceSize;
            let index_len = (index_count as vk::DeviceSize) << index_shift;

            let vertex_offset = align_up_u64(index_len, size_of::<f32>() as vk::DeviceSize);
            let mesh_offset = vertex_offset + vertex_len as vk::DeviceSize;

            let material = mesh_part.material();

            debug_assert!((material as usize) < MAX_MATERIALS_PER_MODEL);

            let mut flags = MeshFlags::empty();
            flags.set(MeshFlags::INDEX_TYPE_UINT32, index_is_u32);
            flags.set(
                MeshFlags::JOINTS_WEIGHTS,
                vertex_ty.contains(Vertex::JOINTS_WEIGHTS),
            );

            let mesh = Mesh {
                index_count,
                index_offset: (self.geometry_len >> index_shift) as _,
                vertex_offset: ((self.geometry_len + vertex_offset)
                    / size_of::<f32>() as vk::DeviceSize) as _,
                vertex_stride: (vertex_stride / size_of::<f32>() as u32) as _,
                material,
                flags,
                _0: Default::default(),
            };

            let temp_len = mesh_offset + Mesh::SIZE;
            let temp_buf = {
                let mut buf = self.pool.lease(BufferInfo::new_mappable(
                    temp_len,
                    vk::BufferUsageFlags::TRANSFER_SRC,
                ))?;

                if index_is_u32 {
                    Buffer::copy_from_slice(&mut buf, 0, cast_slice(&index_buf));
                } else {
                    let index_buf = index_buf
                        .iter()
                        .copied()
                        .map(|idx| idx as u16)
                        .collect::<Box<_>>();
                    Buffer::copy_from_slice(&mut buf, 0, cast_slice(&index_buf));
                };

                Buffer::copy_from_slice(&mut buf, vertex_offset, vertex_buf);
                Buffer::copy_from_slice(&mut buf, mesh_offset, bytes_of(&mesh));

                render_graph.bind_node(buf)
            };

            let dst_mesh_offset = Mesh::SIZE * self.mesh_count as vk::DeviceSize;

            debug_assert!(self.geometry_len + mesh_offset <= self.geometry_buf.info.size);
            debug_assert!(dst_mesh_offset + Mesh::SIZE <= self.mesh_buf.info.size);

            render_graph.copy_buffer_region(
                temp_buf,
                geometry_buf,
                vk::BufferCopy {
                    src_offset: 0,
                    dst_offset: self.geometry_len,
                    size: mesh_offset,
                },
            );
            render_graph.copy_buffer_region(
                temp_buf,
                mesh_buf,
                vk::BufferCopy {
                    src_offset: mesh_offset,
                    dst_offset: dst_mesh_offset,
                    size: Mesh::SIZE,
                },
            );

            geometries.push(Geometry {
                flags,
                index_count,
                index_offset: self.geometry_len,
                vertex_count,
                vertex_offset: self.geometry_len + vertex_offset,
            });

            self.geometry_len += mesh_offset;
            self.geometry_len = align_up_u64(self.geometry_len, size_of::<f32>() as vk::DeviceSize);
            self.mesh_count += 1;
        }

        self.model_count += 1;
        self.technique
            .load_model(&mut render_graph, geometry_buf, &geometries)?;

        render_graph
            .resolve()
            .submit(&mut self.pool, 0, queue_index)?;

        Ok(model)
    }

    fn model_instance_mut(&mut self, model_instance: ModelInstance) -> &mut ModelInstanceData {
        let index = self.model_instance_index[&model_instance];

        &mut self.technique[index]
    }

    pub fn record(
        &mut self,
        render_graph: &mut RenderGraph,
        framebuffer: impl Into<AnyImageNode>,
        camera: &mut Camera,
    ) -> Result<(), DriverError> {
        let framebuffer = framebuffer.into();

        let geometry_buf = render_graph.bind_node(&self.geometry_buf);
        let material_buf = render_graph.bind_node(&self.material_buf);
        let mesh_buf = render_graph.bind_node(&self.mesh_buf);

        self.technique.record(
            render_graph,
            framebuffer,
            camera,
            geometry_buf,
            material_buf,
            mesh_buf,
            &self.textures,
        )
    }

    pub fn remove_model_instance(&mut self, model_instance: ModelInstance) {
        let index = self.model_instance_index.remove(&model_instance).unwrap();
        self.technique.swap_remove_model_instance(index);
        self.model_instances.swap_remove(index);

        if !self.model_instances.is_empty() {
            let model_instance = self.model_instances[index];
            *self.model_instance_index.get_mut(&model_instance).unwrap() = index;
        }

        debug_assert_eq!(self.model_instance_index.len(), self.model_instances.len());
    }

    pub fn set_model_instance_material(
        &mut self,
        model_instance: ModelInstance,
        material_index: usize,
        material: Material,
    ) {
        let model_instance_data = self.model_instance_mut(model_instance);
        model_instance_data.materials[material_index] = material;
    }

    pub fn set_model_instance_materials(
        &mut self,
        model_instance: ModelInstance,
        materials: &[Material],
    ) {
        let model_instance_data = self.model_instance_mut(model_instance);
        model_instance_data.materials = material_array(materials);
    }

    pub fn set_model_instance_transform(
        &mut self,
        model_instance: ModelInstance,
        translation: Vec3,
        rotation: Quat,
    ) {
        let model_instance_data = self.model_instance_mut(model_instance);
        model_instance_data.rotation = rotation;
        model_instance_data.translation = translation;
    }

    pub fn set_model_instance_pose(
        &mut self,
        model_instance: ModelInstance,
        pose: &[(&'static str, Quat)],
    ) {
        let model_instance_data = self.model_instance_mut(model_instance);

        todo!();
    }
}

/// Information used to create a [`ModelBufferInfo`] instance.
#[derive(Builder, Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[builder(
    build_fn(
        private,
        name = "fallible_build",
        error = "ModelBufferInfoBuilderError"
    ),
    derive(Debug),
    pattern = "owned"
)]
pub struct ModelBufferInfo {
    /// Fixed size capacity of the model geometry (indices and vertices) which may be loaded.
    #[builder(default = "10_000_000")]
    pub geometry_capacity: vk::DeviceSize,

    /// Fixed size capacity of individual materials which may be loaded.
    #[builder(default = "1_000")]
    pub material_capacity: vk::DeviceSize,

    /// Fixed size capacity of individual meshes which may be loaded.
    #[builder(default = "5_000")]
    pub mesh_capacity: vk::DeviceSize,

    /// Fixed size capacity of individual models which may be loaded.
    #[builder(default = "5_000")]
    pub model_capacity: vk::DeviceSize,

    /// Technique to use when recording models.
    #[builder(default, setter(strip_option))]
    pub technique: Option<ModelBufferTechnique>,
}

impl ModelBufferInfo {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> ModelBufferInfoBuilder {
        ModelBufferInfoBuilder::default()
    }
}

impl Default for ModelBufferInfo {
    fn default() -> Self {
        ModelBufferInfoBuilder::default().build()
    }
}

// HACK: https://github.com/colin-kiegel/rust-derive-builder/issues/56
impl ModelBufferInfoBuilder {
    /// Builds a new `ModelBufferInfo`.
    pub fn build(self) -> ModelBufferInfo {
        self.fallible_build()
            .expect("All required fields set at initialization")
    }
}

impl From<ModelBufferInfoBuilder> for ModelBufferInfo {
    fn from(info: ModelBufferInfoBuilder) -> Self {
        info.build()
    }
}

#[derive(Debug)]
struct ModelBufferInfoBuilderError;

impl From<UninitializedFieldError> for ModelBufferInfoBuilderError {
    fn from(_: UninitializedFieldError) -> Self {
        Self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum ModelBufferTechnique {
    Raster,
    RayTrace,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ModelInstance(usize);

#[derive(Clone, Copy, Debug)]
struct ModelInstanceData {
    materials: [Material; MAX_MATERIALS_PER_MODEL],
    model: Model,
    rotation: Quat,
    translation: Vec3,
}

trait Technique: Debug + Send + IndexMut<usize> + Index<usize, Output = ModelInstanceData> {
    fn load_model(
        &mut self,
        render_graph: &mut RenderGraph,
        geometry_buf: BufferNode,
        geometries: &[Geometry],
    ) -> Result<(), DriverError>;

    fn push_model_instance(&mut self, model_instance: ModelInstanceData);

    fn record(
        &mut self,
        render_graph: &mut RenderGraph,
        framebuffer: AnyImageNode,
        camera: &mut Camera,
        geometry_buf: BufferNode,
        material_buf: BufferNode,
        mesh_buf: BufferNode,
        textures: &[Arc<Image>],
    ) -> Result<(), DriverError>;

    fn swap_remove_model_instance(&mut self, idx: usize);
}
