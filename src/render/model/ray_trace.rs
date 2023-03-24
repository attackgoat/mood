use {
    super::{
        super::{camera::Camera, lease_storage_buffer},
        sbt::{ShaderBindingGroup, ShaderBindingTable},
        Geometry, Material, Model, ModelBufferInfo, ModelInstanceData, Technique,
        MAX_MATERIALS_PER_MODEL,
    },
    crate::res,
    anyhow::Context,
    bytemuck::{bytes_of, Pod, Zeroable},
    glam::{Mat3, Mat4, Vec3, Vec4},
    screen_13::prelude::*,
    std::{
        ops::{Index, IndexMut},
        sync::Arc,
    },
};

#[cfg(not(feature = "hot-shaders"))]
use super::super::{open_res_pak, read_blob};

#[cfg(feature = "hot-shaders")]
use {super::super::res_shader_dir, screen_13_hot::prelude::*};

fn material_index_array(
    materials: [Material; MAX_MATERIALS_PER_MODEL],
) -> [u32; MAX_MATERIALS_PER_MODEL] {
    let mut res = [0; MAX_MATERIALS_PER_MODEL];
    for idx in 0..MAX_MATERIALS_PER_MODEL {
        res[idx] = materials[idx].material_index as _;
    }

    res
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct ModelInstanceRef {
    material_indices: [u32; MAX_MATERIALS_PER_MODEL],
    mesh_index: u32,
}

#[derive(Debug)]
pub(super) struct RayTrace {
    device: Arc<Device>,
    frame_idx: u32,
    model_blas: Vec<Arc<AccelerationStructure>>,
    model_instances: Vec<ModelInstanceData>,

    #[cfg(not(feature = "hot-shaders"))]
    pipeline: Arc<RayTracePipeline>,

    #[cfg(feature = "hot-shaders")]
    pipeline: HotRayTracePipeline,

    pool: LazyPool,
    sbt: ShaderBindingTable,
}

impl RayTrace {
    pub fn new(device: &Arc<Device>, info: ModelBufferInfo) -> anyhow::Result<Self> {
        #[cfg(not(feature = "hot-shaders"))]
        let mut res_pak = open_res_pak()?;

        #[cfg(feature = "hot-shaders")]
        let shader_dir = res_shader_dir().join("model/ray_trace");

        let shader_groups = [
            RayTraceShaderGroup::new_general(0),
            RayTraceShaderGroup::new_triangles(1, None),
            RayTraceShaderGroup::new_general(2),
            RayTraceShaderGroup::new_general(3),
        ];
        let pipeline_info = RayTracePipelineInfo::new().max_ray_recursion_depth(1);

        let gbuffer_rchit_specialization_info = SpecializationInfo::new(
            [vk::SpecializationMapEntry {
                constant_id: 0,
                offset: 0,
                size: 4,
            }],
            bytes_of(&(MAX_MATERIALS_PER_MODEL as u32)),
        );

        #[cfg(not(feature = "hot-shaders"))]
        let pipeline = Arc::new(
            RayTracePipeline::create(
                &device,
                pipeline_info,
                [
                    Shader::new_ray_gen(
                        read_blob(
                            &mut res_pak,
                            res::SHADER_MODEL_RAY_TRACE_REFERENCE_RGEN_SPIRV,
                        )?
                        .as_slice(),
                    ),
                    Shader::new_closest_hit(
                        read_blob(
                            &mut res_pak,
                            res::SHADER_MODEL_RAY_TRACE_GBUFFER_RCHIT_SPIRV,
                        )?
                        .as_slice(),
                    )
                    .specialization_info(gbuffer_rchit_specialization_info),
                    Shader::new_miss(
                        read_blob(
                            &mut res_pak,
                            res::SHADER_MODEL_RAY_TRACE_GBUFFER_RMISS_SPIRV,
                        )?
                        .as_slice(),
                    ),
                    Shader::new_miss(
                        read_blob(&mut res_pak, res::SHADER_MODEL_RAY_TRACE_SHADOW_RMISS_SPIRV)?
                            .as_slice(),
                    ),
                ],
                shader_groups,
            )
            .context("Creating pipeline")?,
        );

        #[cfg(feature = "hot-shaders")]
        let pipeline = HotRayTracePipeline::create(
            &device,
            pipeline_info,
            [
                HotShader::new_ray_gen(shader_dir.join("reference.rgen")),
                HotShader::new_closest_hit(shader_dir.join("gbuffer.rchit"))
                    .specialization_info(gbuffer_rchit_specialization_info),
                HotShader::new_miss(shader_dir.join("gbuffer.rmiss")),
                HotShader::new_miss(shader_dir.join("shadow.rmiss")),
            ],
            shader_groups,
        )
        .context("Creating hot pipeline")?;

        let sbt = {
            #[cfg(not(feature = "hot-shaders"))]
            let pipeline = &pipeline;

            #[cfg(feature = "hot-shaders")]
            let pipeline = pipeline.cold();

            Self::build_sbt(device, pipeline)?
        };

        let pool = LazyPool::new(device);
        let device = Arc::clone(device);

        Ok(Self {
            device,
            frame_idx: 0,
            model_blas: Default::default(),
            model_instances: Default::default(),
            pipeline,
            pool,
            sbt,
        })
    }

    fn build_blas(
        &mut self,
        render_graph: &mut RenderGraph,
        geometry_buf: BufferNode,
        geometries: &[Geometry],
    ) -> Result<AccelerationStructureNode, DriverError> {
        let geometry_address = render_graph.node_device_address(geometry_buf);
        let geometries = geometries
            .iter()
            .map(|geom| AccelerationStructureGeometry {
                max_primitive_count: geom.index_count / 3,
                flags: vk::GeometryFlagsKHR::OPAQUE,
                geometry: AccelerationStructureGeometryData::Triangles {
                    index_data: DeviceOrHostAddress::DeviceAddress(
                        geometry_address + geom.index_offset,
                    ),
                    index_type: geom.flags.index_ty(),
                    max_vertex: geom.index_count,
                    transform_data: None,
                    vertex_data: DeviceOrHostAddress::DeviceAddress(
                        geometry_address + geom.vertex_offset,
                    ),
                    vertex_format: vk::Format::R32G32B32_SFLOAT,
                    vertex_stride: geom.flags.vertex_stride(),
                },
            })
            .collect();

        let geometry_info = AccelerationStructureGeometryInfo {
            ty: vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL,
            flags: vk::BuildAccelerationStructureFlagsKHR::empty(),
            geometries,
        };
        let blas_size = AccelerationStructure::size_of(&self.device, &geometry_info);
        let blas = render_graph.bind_node(AccelerationStructure::create(
            &self.device,
            AccelerationStructureInfo {
                ty: vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL,
                size: blas_size.create_size,
            },
        )?);

        let accel_struct_scratch_offset_alignment =
            self.device
                .physical_device
                .accel_struct_properties
                .as_ref()
                .unwrap()
                .min_accel_struct_scratch_offset_alignment as vk::DeviceSize;
        let scratch_buf = render_graph.bind_node(
            self.pool.lease(
                BufferInfo::new(
                    blas_size.build_size,
                    vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                        | vk::BufferUsageFlags::STORAGE_BUFFER,
                )
                .alignment(accel_struct_scratch_offset_alignment),
            )?,
        );

        render_graph
            .begin_pass("Build BLAS")
            .access_node(geometry_buf, AccessType::AccelerationStructureBuildRead)
            .access_node(scratch_buf, AccessType::AccelerationStructureBufferWrite)
            .access_node(blas, AccessType::AccelerationStructureBuildWrite)
            .record_acceleration(move |accel, _| {
                let build_ranges = geometry_info
                    .geometries
                    .iter()
                    .map(|geometry| vk::AccelerationStructureBuildRangeInfoKHR {
                        first_vertex: 0,
                        primitive_count: geometry.max_primitive_count,
                        primitive_offset: 0,
                        transform_offset: 0,
                    })
                    .collect::<Box<_>>();

                accel.build_structure(blas, scratch_buf, &geometry_info, &build_ranges);
            });

        Ok(blas)
    }

    fn build_tlas(
        &mut self,
        render_graph: &mut RenderGraph,
    ) -> Result<AccelerationStructureLeaseNode, DriverError> {
        let instances = self
            .model_instances
            .iter()
            .enumerate()
            .map(|(model_instance_index, model_instance_data)| {
                let Model { model_idx, .. } = model_instance_data.model;
                let blas = &self.model_blas[model_idx];
                let mut matrix = [0.0; 12];
                matrix.copy_from_slice(
                    &Mat4::from_rotation_translation(
                        model_instance_data.rotation,
                        model_instance_data.translation,
                    )
                    .transpose()
                    .to_cols_array()[0..12],
                );

                vk::AccelerationStructureInstanceKHR {
                    transform: vk::TransformMatrixKHR { matrix },
                    instance_custom_index_and_mask: vk::Packed24_8::new(
                        model_instance_index as _,
                        0xff,
                    ),
                    instance_shader_binding_table_record_offset_and_flags: vk::Packed24_8::new(
                        0,
                        vk::GeometryInstanceFlagsKHR::FORCE_OPAQUE.as_raw() as _,
                    ),
                    acceleration_structure_reference: vk::AccelerationStructureReferenceKHR {
                        device_handle: AccelerationStructure::device_address(blas),
                    },
                }
            })
            .collect::<Box<_>>();
        let instance_count = instances.len() as _;
        let instance_data = AccelerationStructure::instance_slice(&instances);
        let mut instance_buf = self.pool.lease(BufferInfo::new_mappable(
            instance_data.len() as _,
            vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
        ))?;

        Buffer::copy_from_slice(&mut instance_buf, 0, instance_data);

        let geometry_info = AccelerationStructureGeometryInfo {
            ty: vk::AccelerationStructureTypeKHR::TOP_LEVEL,
            flags: vk::BuildAccelerationStructureFlagsKHR::empty(),
            geometries: vec![AccelerationStructureGeometry {
                max_primitive_count: instance_count,
                flags: vk::GeometryFlagsKHR::OPAQUE,
                geometry: AccelerationStructureGeometryData::Instances {
                    array_of_pointers: false,
                    data: DeviceOrHostAddress::DeviceAddress(Buffer::device_address(&instance_buf)),
                },
            }],
        };
        let tlas_size = AccelerationStructure::size_of(&self.device, &geometry_info);
        let tlas = self.pool.lease(AccelerationStructureInfo {
            ty: vk::AccelerationStructureTypeKHR::TOP_LEVEL,
            size: tlas_size.create_size,
        })?;

        let accel_struct_scratch_offset_alignment =
            self.device
                .physical_device
                .accel_struct_properties
                .as_ref()
                .unwrap()
                .min_accel_struct_scratch_offset_alignment as vk::DeviceSize;

        let instance_buf = render_graph.bind_node(instance_buf);
        let scratch_buf = render_graph.bind_node(
            self.pool.lease(
                BufferInfo::new(
                    tlas_size.build_size,
                    vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                        | vk::BufferUsageFlags::STORAGE_BUFFER,
                )
                .alignment(accel_struct_scratch_offset_alignment),
            )?,
        );
        let tlas = render_graph.bind_node(tlas);

        let mut pass = render_graph.begin_pass("Build TLAS");

        for blas in &self.model_blas {
            let blas = pass.bind_node(blas);
            pass.access_node_mut(blas, AccessType::AccelerationStructureBuildRead);
        }

        pass.access_node(instance_buf, AccessType::AccelerationStructureBuildRead)
            .access_node(scratch_buf, AccessType::AccelerationStructureBufferWrite)
            .access_node(tlas, AccessType::AccelerationStructureBuildWrite)
            .record_acceleration(move |accel, _| {
                accel.build_structure(
                    tlas,
                    scratch_buf,
                    &geometry_info,
                    &[vk::AccelerationStructureBuildRangeInfoKHR {
                        first_vertex: 0,
                        primitive_count: instance_count,
                        primitive_offset: 0,
                        transform_offset: 0,
                    }],
                );
            });

        Ok(tlas)
    }

    fn build_sbt(
        device: &Arc<Device>,
        pipeline: &Arc<RayTracePipeline>,
    ) -> Result<ShaderBindingTable, DriverError> {
        ShaderBindingTable::new(
            device,
            pipeline,
            ShaderBindingGroup::new(1, 1),
            ShaderBindingGroup::new(2, 1),
            None,
        )
    }
}

impl Index<usize> for RayTrace {
    type Output = ModelInstanceData;

    fn index(&self, index: usize) -> &Self::Output {
        &self.model_instances[index]
    }
}

impl IndexMut<usize> for RayTrace {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.model_instances[index]
    }
}

impl Technique for RayTrace {
    fn load_model(
        &mut self,
        render_graph: &mut RenderGraph,
        geometry_buf: BufferNode,
        geometries: &[Geometry],
    ) -> Result<(), DriverError> {
        let blas = self.build_blas(render_graph, geometry_buf, geometries)?;
        let blas = render_graph.unbind_node(blas);

        self.model_blas.push(blas);

        Ok(())
    }

    fn push_model_instance(&mut self, model_instance: ModelInstanceData) {
        self.model_instances.push(model_instance);
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
        // TODO: Rebuild these two only when needed
        let tlas = self.build_tlas(render_graph)?;
        let model_instances_buf = render_graph.bind_node(lease_storage_buffer(
            &mut self.pool,
            &self
                .model_instances
                .iter()
                .map(|model_instance| ModelInstanceRef {
                    material_indices: material_index_array(model_instance.materials),
                    mesh_index: model_instance.model.mesh_idx as _,
                })
                .collect::<Box<_>>(),
        )?);

        #[cfg(not(feature = "hot-shaders"))]
        let pipeline = &self.pipeline;

        #[cfg(feature = "hot-shaders")]
        let pipeline = self.pipeline.hot();

        #[cfg(feature = "hot-shaders")]
        // Shader binding table becomes invalid if the pipeline is recompiled
        if !self.sbt.is_valid(pipeline) {
            self.sbt = Self::build_sbt(&self.device, pipeline)?;
        }

        let sbt = render_graph.bind_node(&self.sbt.buffer);
        let (
            raygen_shader_binding_tables,
            hit_shader_binding_tables,
            miss_shader_binding_tables,
            callable_shader_binding_tables,
        ) = self.sbt.regions();

        let mut pass = render_graph
            .begin_pass("Reference path trace")
            .bind_pipeline(pipeline)
            .access_node(sbt, AccessType::RayTracingShaderReadOther)
            .write_descriptor(0, framebuffer)
            .access_descriptor(
                1,
                tlas,
                AccessType::RayTracingShaderReadAccelerationStructure,
            )
            .access_descriptor(2, geometry_buf, AccessType::RayTracingShaderReadOther)
            .access_descriptor(3, material_buf, AccessType::RayTracingShaderReadOther)
            .access_descriptor(4, mesh_buf, AccessType::RayTracingShaderReadOther)
            .access_descriptor(
                6,
                model_instances_buf,
                AccessType::RayTracingShaderReadOther,
            );

        for (idx, texture) in textures.iter().enumerate() {
            let texture = pass.bind_node(texture);
            pass = pass.read_descriptor((7, [idx as u32]), texture);
        }

        let view = Mat3::from_rotation_y(camera.yaw.to_radians())
            * Mat3::from_rotation_x(camera.pitch.to_radians());
        let view = view.to_cols_array_2d();
        let view = [
            Vec3::from_array(view[0]).extend(0.0),
            Vec3::from_array(view[1]).extend(0.0),
            Vec3::from_array(view[2]).extend(0.0),
        ];

        #[derive(Clone, Copy, Pod, Zeroable)]
        #[repr(C)]
        struct PushConstants {
            view: [Vec4; 3],
            view_position: Vec3,
            aspect_ratio: f32,
            fov_y: f32, // in radians
            frame_index: u32,
            _0: [u8; 8],
        }

        let push_consts = PushConstants {
            aspect_ratio: camera.aspect_ratio,
            fov_y: camera.fov_y.to_radians(),
            frame_index: self.frame_idx,
            view_position: camera.position,
            view,
            _0: Default::default(),
        };
        let ImageInfo { width, height, .. } = pass.node_info(framebuffer);

        pass.record_ray_trace(move |ray_trace, _| {
            ray_trace.push_constants(bytes_of(&push_consts)).trace_rays(
                &raygen_shader_binding_tables,
                &miss_shader_binding_tables,
                &hit_shader_binding_tables,
                &callable_shader_binding_tables,
                width,
                height,
                1,
            );
        });

        self.frame_idx = self.frame_idx.wrapping_add(1);

        Ok(())
    }

    fn swap_remove_model_instance(&mut self, idx: usize) {
        self.model_instances.swap_remove(idx);
    }
}
