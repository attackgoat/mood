use {
    crate::{math::align_up_u32, res},
    anyhow::Context,
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
pub struct ExclusiveSumPipeline {
    reduce: Arc<ComputePipeline>,
    scan: Arc<ComputePipeline>,
    subgroup_size: u32,
}

#[cfg(feature = "hot-shaders")]
#[derive(Debug)]
pub struct ExclusiveSumPipeline {
    reduce: HotComputePipeline,
    scan: HotComputePipeline,
    subgroup_size: u32,
}

impl ExclusiveSumPipeline {
    #[cfg(not(feature = "hot-shaders"))]
    pub fn new(device: &Arc<Device>, res_pak: &mut PakBuf) -> anyhow::Result<Self> {
        let Vulkan11Properties { subgroup_size, .. } = device.physical_device.properties_v1_1;

        let reduce = Arc::new(
            ComputePipeline::create(
                &device,
                ComputePipelineInfo::default(),
                Shader::new_compute(
                    read_blob(res_pak, res::SHADER_COMPUTE_EXCL_SUM_REDUCE_COMP_SPIRV)?.as_slice(),
                )
                .specialization_info(Self::subgroup_specialization_info(subgroup_size)),
            )
            .context("Creating reduce pipeline")?,
        );

        let scan = Arc::new(
            ComputePipeline::create(
                &device,
                ComputePipelineInfo::default(),
                Shader::new_compute(
                    read_blob(res_pak, res::SHADER_COMPUTE_EXCL_SUM_SCAN_COMP_SPIRV)?.as_slice(),
                )
                .specialization_info(Self::subgroup_specialization_info(subgroup_size)),
            )
            .context("Creating scan pipeline")?,
        );

        Ok(Self {
            reduce,
            scan,
            subgroup_size,
        })
    }

    #[cfg(feature = "hot-shaders")]
    pub fn new(device: &Arc<Device>) -> anyhow::Result<Self> {
        let PhysicalDeviceVulkan11Properties { subgroup_size, .. } = device.vulkan_1_1_properties;
        let shader_dir = res_shader_dir();

        let reduce = HotComputePipeline::create(
            &device,
            ComputePipelineInfo::default(),
            HotShader::new_compute(shader_dir.join("compute/excl_sum_reduce.comp"))
                .specialization_info(Self::subgroup_specialization_info(subgroup_size)),
        )
        .context("Creating hot reduce pipeline")?;

        let scan = HotComputePipeline::create(
            &device,
            ComputePipelineInfo::default(),
            HotShader::new_compute(shader_dir.join("compute/excl_sum_scan.comp"))
                .specialization_info(Self::subgroup_specialization_info(subgroup_size)),
        )
        .context("Creating hot scan pipeline")?;

        Ok(Self {
            reduce,
            scan,
            subgroup_size,
        })
    }

    pub fn align_input_count(&self, input_count: u32) -> u32 {
        align_up_u32(input_count, self.subgroup_size)
    }

    pub fn record(
        &mut self,
        render_graph: &mut RenderGraph,
        pool: &mut impl Pool<BufferInfoBuilder, Buffer>,
        input_buf: impl Into<AnyBufferNode>,
        input_count: u32,
        output_buf: impl Into<AnyBufferNode>,
    ) -> Result<(), DriverError> {
        if input_count == 0 {
            return Ok(());
        }

        debug_assert!(
            input_count % self.subgroup_size == 0,
            "Input count is expected to be manually aligned to subgroup size"
        );

        let input_buf = input_buf.into();
        let output_buf = output_buf.into();

        let workgroup_count = input_count / self.subgroup_size;
        let reduce_count = workgroup_count - 1;
        let workgroup_buf = render_graph.bind_node(pool.lease(BufferInfo::new(
            reduce_count.max(1) as vk::DeviceSize * size_of::<u32>() as vk::DeviceSize,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        ))?);

        if reduce_count > 0 {
            render_graph
                .begin_pass("exclusive sum reduce")
                .bind_pipeline(self.reduce())
                .read_descriptor(0, input_buf)
                .write_descriptor(1, workgroup_buf)
                .record_compute(move |compute, _| {
                    compute.dispatch(reduce_count, 1, 1);
                });
        }

        render_graph
            .begin_pass("exclusive sum scan")
            .bind_pipeline(self.scan())
            .read_descriptor(0, workgroup_buf)
            .read_descriptor(1, input_buf)
            .write_descriptor(2, output_buf)
            .record_compute(move |compute, _| {
                compute.dispatch(workgroup_count, 1, 1);
            });

        Ok(())
    }

    #[inline(always)]
    fn reduce(&mut self) -> &Arc<ComputePipeline> {
        #[cfg(not(feature = "hot-shaders"))]
        let res = &self.reduce;

        #[cfg(feature = "hot-shaders")]
        let res = self.reduce.hot();

        res
    }

    #[inline(always)]
    fn scan(&mut self) -> &Arc<ComputePipeline> {
        #[cfg(not(feature = "hot-shaders"))]
        let res = &self.scan;

        #[cfg(feature = "hot-shaders")]
        let res = self.scan.hot();

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
        bytemuck::cast_slice,
        rand::{rngs::SmallRng, Rng, SeedableRng},
        std::{
            iter::{repeat, repeat_with},
            mem::size_of,
            sync::Arc,
        },
    };

    #[cfg(not(feature = "hot-shaders"))]
    use super::super::open_res_pak;

    fn assert_exclusive_sum(input_data: &[u32]) {
        let device = Arc::new(Device::create_headless(DeviceInfo::new()).unwrap());
        let mut pool = LazyPool::new(&device);

        #[cfg(not(feature = "hot-shaders"))]
        let mut excl_sum_pipeline = {
            let mut res_pak = open_res_pak().unwrap();

            ExclusiveSumPipeline::new(&device, &mut res_pak).unwrap()
        };

        #[cfg(feature = "hot-shaders")]
        let mut excl_sum_pipeline = ExclusiveSumPipeline::new(&device).unwrap();

        // Trim input data because we expect applications to always provide data in multiples of
        // device subgroup size
        let input_data = &input_data[0..(input_data.len()
            / excl_sum_pipeline.subgroup_size as usize)
            * excl_sum_pipeline.subgroup_size as usize];

        let mut render_graph = RenderGraph::new();

        let input_count = input_data.len() as u32;
        let input_buf = render_graph.bind_node(Arc::new(
            Buffer::create_from_slice(
                &device,
                vk::BufferUsageFlags::STORAGE_BUFFER,
                cast_slice(&input_data),
            )
            .unwrap(),
        ));
        let output_buf = render_graph.bind_node(Arc::new(
            Buffer::create(
                &device,
                BufferInfo::new_mappable(
                    input_count as vk::DeviceSize * size_of::<u32>() as vk::DeviceSize,
                    vk::BufferUsageFlags::STORAGE_BUFFER,
                ),
            )
            .unwrap(),
        ));

        excl_sum_pipeline
            .record(
                &mut render_graph,
                &mut pool,
                input_buf,
                input_count,
                output_buf,
            )
            .unwrap();

        let output_buf = render_graph.unbind_node(output_buf);

        render_graph
            .resolve()
            .submit(&mut LazyPool::new(&device), 0, 0)
            .unwrap()
            .wait_until_executed()
            .unwrap();

        let output_data: &[u32] = cast_slice(Buffer::mapped_slice(&output_buf));

        assert_eq!(output_data.len(), input_count as usize);

        let mut sum = 0;
        for idx in 0..input_count as usize {
            assert_eq!(sum, output_data[idx]);

            sum += input_data[idx];
        }
    }

    #[cfg_attr(not(target_os = "macos"), test)]
    pub fn exclusive_sum1() {
        let input_data = (0u32..2_048).into_iter().collect::<Box<_>>();

        assert_exclusive_sum(&input_data);
    }

    #[cfg_attr(not(target_os = "macos"), test)]
    pub fn exclusive_sum2() {
        let input_data = (0u32..69).into_iter().collect::<Box<_>>();

        assert_exclusive_sum(&input_data);
    }

    #[cfg_attr(not(target_os = "macos"), test)]
    pub fn exclusive_sum3() {
        let input_data = repeat(1u32).take(99_048).into_iter().collect::<Box<_>>();

        assert_exclusive_sum(&input_data);
    }

    #[cfg_attr(not(target_os = "macos"), test)]
    pub fn exclusive_sum4() {
        let mut rng = SmallRng::seed_from_u64(42);
        let input_data = repeat_with(|| rng.gen_range(0u32..35))
            .take(16_123)
            .collect::<Box<_>>();

        assert_exclusive_sum(&input_data);
    }
}
