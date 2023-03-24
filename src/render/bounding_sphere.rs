use {
    crate::res,
    anyhow::Context,
    bytemuck::{bytes_of, Pod, Zeroable},
    glam::{Vec3, Vec4},
    pak::PakBuf,
    screen_13::prelude::*,
    std::{mem::size_of, sync::Arc},
};

#[cfg(not(feature = "hot-shaders"))]
use super::read_blob;

#[cfg(feature = "hot-shaders")]
use {super::res_shader_dir, screen_13_hot::prelude::*};

#[cfg(not(feature = "hot-shaders"))]
#[derive(Debug)]
pub struct BoundingSpherePipeline {
    avg: Arc<ComputePipeline>,
    dist_sq: Arc<ComputePipeline>,
    reduce_avg: Arc<ComputePipeline>,
    reduce_dist_sq: Arc<ComputePipeline>,
    subgroup_size: u32,
}

#[cfg(feature = "hot-shaders")]
#[derive(Debug)]
pub struct BoundingSpherePipeline {
    avg: HotComputePipeline,
    dist_sq: HotComputePipeline,
    reduce_avg: HotComputePipeline,
    reduce_dist_sq: HotComputePipeline,
    subgroup_size: u32,
}

impl BoundingSpherePipeline {
    #[cfg(not(feature = "hot-shaders"))]
    pub fn new(device: &Arc<Device>, res_pak: &mut PakBuf) -> anyhow::Result<Self> {
        let Vulkan11Properties { subgroup_size, .. } = device.physical_device.properties_v1_1;

        let avg = Arc::new(
            ComputePipeline::create(
                &device,
                ComputePipelineInfo::default(),
                Shader::new_compute(
                    read_blob(res_pak, res::SHADER_COMPUTE_BOUNDING_SPHERE_AVG_COMP_SPIRV)?
                        .as_slice(),
                )
                .specialization_info(Self::subgroup_specialization_info(subgroup_size)),
            )
            .context("Creating average pipeline")?,
        );

        let dist_sq = Arc::new(
            ComputePipeline::create(
                &device,
                ComputePipelineInfo::default(),
                Shader::new_compute(
                    read_blob(
                        res_pak,
                        res::SHADER_COMPUTE_BOUNDING_SPHERE_DIST_SQ_COMP_SPIRV,
                    )?
                    .as_slice(),
                )
                .specialization_info(Self::subgroup_specialization_info(subgroup_size)),
            )
            .context("Creating distance squared pipeline")?,
        );

        let reduce_avg = Arc::new(
            ComputePipeline::create(
                &device,
                ComputePipelineInfo::default(),
                Shader::new_compute(
                    read_blob(
                        res_pak,
                        res::SHADER_COMPUTE_BOUNDING_SPHERE_REDUCE_AVG_COMP_SPIRV,
                    )?
                    .as_slice(),
                )
                .specialization_info(Self::subgroup_specialization_info(subgroup_size)),
            )
            .context("Creating reduce average pipeline")?,
        );

        let reduce_dist_sq = Arc::new(
            ComputePipeline::create(
                &device,
                ComputePipelineInfo::default(),
                Shader::new_compute(
                    read_blob(
                        res_pak,
                        res::SHADER_COMPUTE_BOUNDING_SPHERE_REDUCE_DIST_SQ_COMP_SPIRV,
                    )?
                    .as_slice(),
                )
                .specialization_info(Self::subgroup_specialization_info(subgroup_size)),
            )
            .context("Creating reduce distance squared pipeline")?,
        );

        Ok(Self {
            avg,
            dist_sq,
            reduce_avg,
            reduce_dist_sq,
            subgroup_size,
        })
    }

    #[cfg(feature = "hot-shaders")]
    pub fn new(device: &Arc<Device>) -> anyhow::Result<Self> {
        let PhysicalDeviceVulkan11Properties { subgroup_size, .. } = device.vulkan_1_1_properties;
        let shader_dir = res_shader_dir();

        let avg = HotComputePipeline::create(
            &device,
            ComputePipelineInfo::default(),
            HotShader::new_compute(shader_dir.join("compute/bounding_sphere_avg.comp"))
                .specialization_info(Self::subgroup_specialization_info(subgroup_size)),
        )
        .context("Creating hot average pipeline")?;

        let dist_sq = HotComputePipeline::create(
            &device,
            ComputePipelineInfo::default(),
            HotShader::new_compute(shader_dir.join("compute/bounding_sphere_dist_sq.comp"))
                .specialization_info(Self::subgroup_specialization_info(subgroup_size)),
        )
        .context("Creating hot distance squared pipeline")?;

        let reduce_avg = HotComputePipeline::create(
            &device,
            ComputePipelineInfo::default(),
            HotShader::new_compute(shader_dir.join("compute/bounding_sphere_reduce_avg.comp"))
                .specialization_info(Self::subgroup_specialization_info(subgroup_size)),
        )
        .context("Creating hot reduce average pipeline")?;

        let reduce_dist_sq = HotComputePipeline::create(
            &device,
            ComputePipelineInfo::default(),
            HotShader::new_compute(shader_dir.join("compute/bounding_sphere_reduce_dist_sq.comp"))
                .specialization_info(Self::subgroup_specialization_info(subgroup_size)),
        )
        .context("Creating hot reduce distance squared pipeline")?;

        Ok(Self {
            avg,
            dist_sq,
            reduce_avg,
            reduce_dist_sq,
            subgroup_size,
        })
    }

    #[inline(always)]
    fn avg(&mut self) -> &Arc<ComputePipeline> {
        #[cfg(not(feature = "hot-shaders"))]
        let res = &self.avg;

        #[cfg(feature = "hot-shaders")]
        let res = self.avg.hot();

        res
    }

    #[inline(always)]
    fn dist_sq(&mut self) -> &Arc<ComputePipeline> {
        #[cfg(not(feature = "hot-shaders"))]
        let res = &self.dist_sq;

        #[cfg(feature = "hot-shaders")]
        let res = self.dist_sq.hot();

        res
    }

    pub fn record(
        &mut self,
        render_graph: &mut RenderGraph,
        pool: &mut impl Pool<BufferInfoBuilder, Buffer>,
        vertex_buf: impl Into<AnyBufferNode>,
        vertex_count: u32,
        vertex_offset: u32,
        vertex_stride: u32,
        bounding_sphere_buf: impl Into<AnyBufferNode>,
        bounding_sphere_offset: vk::DeviceSize,
    ) -> Result<(), DriverError> {
        debug_assert_ne!(vertex_count, 0);

        let vertex_buf = vertex_buf.into();
        let bounding_sphere_buf = bounding_sphere_buf.into();

        let vertex_len = vertex_count * vertex_stride * size_of::<f32>() as u32;

        let workgroup_count = (vertex_count + self.subgroup_size - 1) / self.subgroup_size;
        let reduce_count = (workgroup_count + self.subgroup_size - 1) / self.subgroup_size;

        let avg_workgroup_buf = render_graph.bind_node(pool.lease(BufferInfo::new(
            workgroup_count as vk::DeviceSize * size_of::<Vec4>() as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::TRANSFER_SRC
                | vk::BufferUsageFlags::UNIFORM_BUFFER,
        ))?);
        let avg_reduce_buf = render_graph.bind_node(pool.lease(BufferInfo::new(
            reduce_count as vk::DeviceSize * size_of::<Vec4>() as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::TRANSFER_SRC
                | vk::BufferUsageFlags::UNIFORM_BUFFER,
        ))?);
        let dist_sq_workgroup_buf = render_graph.bind_node(pool.lease(BufferInfo::new(
            workgroup_count as vk::DeviceSize * size_of::<f32>() as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_SRC,
        ))?);
        let dist_sq_reduce_buf = render_graph.bind_node(pool.lease(BufferInfo::new(
            reduce_count as vk::DeviceSize * size_of::<f32>() as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_SRC,
        ))?);

        #[derive(Clone, Copy, Pod, Zeroable)]
        #[repr(C)]
        struct VertexPushConstants {
            vertex_count: u32,
            vertex_offset: u32,
            vertex_stride: u32,
        }

        render_graph
            .begin_pass("bounding sphere average")
            .bind_pipeline(self.avg())
            .read_descriptor(0, vertex_buf)
            .write_descriptor(1, avg_workgroup_buf)
            .record_compute(move |compute, _| {
                compute
                    .push_constants(bytes_of(&VertexPushConstants {
                        vertex_count,
                        vertex_offset,
                        vertex_stride,
                    }))
                    .dispatch(workgroup_count, 1, 1);
            });

        let avg_buf = {
            let (mut input_buf, mut output_buf) = (avg_workgroup_buf, avg_reduce_buf);
            let mut reduce_count = workgroup_count;

            while reduce_count > 1 {
                let input_len = reduce_count;
                reduce_count = (reduce_count + self.subgroup_size - 1) / self.subgroup_size;

                render_graph
                    .begin_pass("bounding sphere reduce average")
                    .bind_pipeline(self.reduce_avg())
                    .read_descriptor(0, input_buf)
                    .write_descriptor(1, output_buf)
                    .record_compute(move |compute, _| {
                        compute.push_constants(&input_len.to_ne_bytes()).dispatch(
                            reduce_count,
                            1,
                            1,
                        );
                    });

                (input_buf, output_buf) = (output_buf, input_buf);
            }

            input_buf
        };

        render_graph.copy_buffer_region(
            avg_buf,
            bounding_sphere_buf,
            &vk::BufferCopy {
                src_offset: 0,
                dst_offset: bounding_sphere_offset,
                size: size_of::<Vec3>() as _,
            },
        );

        render_graph
            .begin_pass("bounding sphere distance squared")
            .bind_pipeline(self.dist_sq())
            .read_descriptor(0, vertex_buf)
            .read_descriptor_as(1, avg_buf, 0..size_of::<Vec3>() as _)
            .write_descriptor(2, dist_sq_workgroup_buf)
            .record_compute(move |compute, _| {
                compute
                    .push_constants(bytes_of(&VertexPushConstants {
                        vertex_count,
                        vertex_offset,
                        vertex_stride,
                    }))
                    .dispatch(workgroup_count, 1, 1);
            });

        let dist_sq_buf = {
            let (mut input_buf, mut output_buf) = (dist_sq_workgroup_buf, dist_sq_reduce_buf);
            let mut reduce_count = workgroup_count;

            while reduce_count > 1 {
                let input_len = reduce_count;
                reduce_count = (reduce_count + self.subgroup_size - 1) / self.subgroup_size;

                render_graph
                    .begin_pass("bounding sphere reduce distance squared")
                    .bind_pipeline(self.reduce_dist_sq())
                    .read_descriptor(0, input_buf)
                    .write_descriptor(1, output_buf)
                    .record_compute(move |compute, _| {
                        compute.push_constants(&input_len.to_ne_bytes()).dispatch(
                            reduce_count,
                            1,
                            1,
                        );
                    });

                (input_buf, output_buf) = (output_buf, input_buf);
            }

            input_buf
        };

        render_graph.copy_buffer_region(
            dist_sq_buf,
            bounding_sphere_buf,
            &vk::BufferCopy {
                src_offset: 0,
                dst_offset: bounding_sphere_offset + size_of::<Vec3>() as vk::DeviceSize,
                size: size_of::<f32>() as _,
            },
        );

        Ok(())
    }

    #[inline(always)]
    fn reduce_avg(&mut self) -> &Arc<ComputePipeline> {
        #[cfg(not(feature = "hot-shaders"))]
        let res = &self.reduce_avg;

        #[cfg(feature = "hot-shaders")]
        let res = self.reduce_avg.hot();

        res
    }

    #[inline(always)]
    fn reduce_dist_sq(&mut self) -> &Arc<ComputePipeline> {
        #[cfg(not(feature = "hot-shaders"))]
        let res = &self.reduce_dist_sq;

        #[cfg(feature = "hot-shaders")]
        let res = self.reduce_dist_sq.hot();

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

#[cfg(test)]
pub(super) mod tests {
    use {
        super::*,
        bytemuck::{cast_slice, from_bytes, NoUninit, Pod, Zeroable},
        glam::{vec3, Vec3},
        rand::{rngs::SmallRng, Rng, SeedableRng},
        std::{iter::repeat_with, mem::size_of, sync::Arc},
    };

    #[cfg(not(feature = "hot-shaders"))]
    use super::super::open_res_pak;

    trait F32Ext {
        fn abs_diff_eq(self, rhs: Self, max_abs_diff: f32) -> bool;
    }

    impl F32Ext for f32 {
        fn abs_diff_eq(self, rhs: Self, max_abs_diff: f32) -> bool {
            (self - rhs).abs() <= max_abs_diff
        }
    }

    fn assert_bounding_sphere<T>(
        vertices: &[T],
        expected_center: Vec3,
        expected_radius: f32,
        max_diff: f32,
    ) where
        T: NoUninit,
    {
        let device = Arc::new(Device::create_headless(DeviceInfo::new()).unwrap());
        let mut pool = LazyPool::new(&device);

        #[cfg(not(feature = "hot-shaders"))]
        let mut bounding_sphere_pipeline = {
            let mut res_pak = open_res_pak().unwrap();

            BoundingSpherePipeline::new(&device, &mut res_pak).unwrap()
        };

        #[cfg(feature = "hot-shaders")]
        let mut bounding_sphere_pipeline = BoundingSpherePipeline::new(&device).unwrap();
        let mut render_graph = RenderGraph::new();

        let vertex_count = vertices.len() as u32;
        let vertex_offset = 0;
        let vertex_stride = (size_of::<T>() / size_of::<f32>()) as u32;
        let vertex_buf = render_graph.bind_node(Arc::new(
            Buffer::create_from_slice(
                &device,
                vk::BufferUsageFlags::STORAGE_BUFFER,
                cast_slice(&vertices),
            )
            .unwrap(),
        ));

        let bounding_sphere_offset = 2048;
        let bounding_sphere_buf = render_graph.bind_node(Arc::new(
            Buffer::create(
                &device,
                BufferInfo::new_mappable(
                    8192,
                    vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
                ),
            )
            .unwrap(),
        ));

        bounding_sphere_pipeline
            .record(
                &mut render_graph,
                &mut pool,
                vertex_buf,
                vertex_count,
                vertex_offset,
                vertex_stride,
                bounding_sphere_buf,
                bounding_sphere_offset,
            )
            .unwrap();

        let bounding_sphere_buf = render_graph.unbind_node(bounding_sphere_buf);

        render_graph
            .resolve()
            .submit(&mut pool, 0, 0)
            .unwrap()
            .wait_until_executed()
            .unwrap();

        #[derive(Clone, Copy, Pod, Zeroable)]
        #[repr(C)]
        struct BoundingSphere {
            position: Vec3,
            radius: f32,
        }

        let final_data: &BoundingSphere = from_bytes(
            &Buffer::mapped_slice(&bounding_sphere_buf)[bounding_sphere_offset as usize
                ..bounding_sphere_offset as usize + size_of::<BoundingSphere>()],
        );

        assert!(
            final_data.position.abs_diff_eq(expected_center, max_diff),
            "{} != {}",
            final_data.position,
            expected_center
        );
        assert!(
            final_data.radius.abs_diff_eq(expected_radius, max_diff),
            "{} != {}",
            final_data.radius,
            expected_radius
        );
    }

    #[cfg_attr(not(target_os = "macos"), test)]
    pub fn bounding_sphere1() {
        let mut rng = SmallRng::seed_from_u64(42);
        let vertices = repeat_with(|| {
            let normal = vec3(
                rng.gen_range(-1.0..=1.0),
                rng.gen_range(-1.0..=1.0),
                rng.gen_range(-1.0..=1.0),
            )
            .normalize();
            let position = normal * rng.gen_range(0.5..=1.0);

            position.to_array()
        })
        .take(2_912_434)
        .collect::<Box<_>>();

        assert_bounding_sphere(&vertices, Vec3::ZERO, 1.0, 0.01);
    }

    #[cfg_attr(not(target_os = "macos"), test)]
    pub fn bounding_sphere2() {
        let mut vertices = repeat_with(|| [0f32, 0.0, 0.0])
            .take(29)
            .collect::<Box<_>>();

        vertices[4] = vec3(-2.0, 0.0, 0.0).to_array();
        vertices[23] = vec3(2.0, 0.0, 0.0).to_array();

        assert_bounding_sphere(&vertices, Vec3::ZERO, 4.0, 0.0001);
    }

    #[cfg_attr(not(target_os = "macos"), test)]
    pub fn bounding_sphere3() {
        let vertices = [
            vec3(2.0, 1.0, -1.0).to_array(),
            vec3(6.0, 1.0, -1.0).to_array(),
        ];

        assert_bounding_sphere(&vertices, vec3(4.0, 1.0, -1.0), 4.0, 0.0001);
    }
}
