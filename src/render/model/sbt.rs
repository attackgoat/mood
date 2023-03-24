use {super::align_up_u32, screen_13::prelude::*, std::sync::Arc};

#[derive(Clone, Copy, Debug)]
pub struct ShaderBindingGroup {
    pub group_index: usize,
    pub shader_count: u32,
}

impl ShaderBindingGroup {
    pub fn new(group_index: usize, shader_count: u32) -> Self {
        Self {
            group_index,
            shader_count,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ShaderBindingTable {
    pub buffer: Arc<Buffer>,
    _pipeline: Arc<RayTracePipeline>,

    ray_gen: vk::StridedDeviceAddressRegionKHR,
    hit: vk::StridedDeviceAddressRegionKHR,
    miss: vk::StridedDeviceAddressRegionKHR,
    callable: vk::StridedDeviceAddressRegionKHR,
}

impl ShaderBindingTable {
    pub fn new(
        device: &Arc<Device>,
        pipeline: &Arc<RayTracePipeline>,
        hit: ShaderBindingGroup,
        miss: ShaderBindingGroup,
        callable: Option<ShaderBindingGroup>,
    ) -> Result<Self, DriverError> {
        let &RayTraceProperties {
            shader_group_base_alignment,
            shader_group_handle_alignment,
            shader_group_handle_size,
            ..
        } = device
            .physical_device
            .ray_trace_properties
            .as_ref()
            .ok_or(DriverError::Unsupported)?;

        let handle_size = align_up_u32(shader_group_handle_size, shader_group_base_alignment);
        let group_size = align_up_u32(handle_size, shader_group_base_alignment);
        let ray_gen_size = group_size;
        let hit_size = group_size * hit.shader_count;
        let miss_size = group_size * miss.shader_count;
        let callable_size =
            group_size * callable.map(|group| group.shader_count).unwrap_or_default();

        // TODO: Use device-only buffer
        let mut buffer = Buffer::create(
            &device,
            BufferInfo::new_mappable(
                (ray_gen_size + hit_size + miss_size + callable_size) as _,
                vk::BufferUsageFlags::SHADER_BINDING_TABLE_KHR
                    | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            )
            .alignment(shader_group_base_alignment as _),
        )?;

        let data = Buffer::mapped_slice_mut(&mut buffer);
        data.fill(0);

        Self::copy_group_handle(pipeline, data, 0, group_size)?;

        let hit_offset = Self::copy_group_handle(pipeline, data, hit.group_index, group_size)?;
        let miss_offset = Self::copy_group_handle(pipeline, data, miss.group_index, group_size)?;
        let callable_offset = callable
            .map(|group| Self::copy_group_handle(pipeline, data, group.group_index, group_size))
            .transpose()?
            .unwrap_or_default();

        let buffer = Arc::new(buffer);
        let device_address = Buffer::device_address(&buffer);

        let ray_gen = vk::StridedDeviceAddressRegionKHR {
            device_address,
            stride: handle_size as _,
            size: ray_gen_size as _,
        };
        let hit = vk::StridedDeviceAddressRegionKHR {
            device_address: device_address + ray_gen_size as vk::DeviceAddress,
            stride: handle_size as _,
            size: hit_size as _,
        };
        let miss = vk::StridedDeviceAddressRegionKHR {
            device_address: device_address + hit_size as vk::DeviceAddress,
            stride: group_size as _,
            size: miss_size as _,
        };
        // let callable = vk::StridedDeviceAddressRegionKHR {
        //     device_address: device_address + callable_offset,
        //     stride: group_size as _,
        //     size: callable_size as _,
        // };
        let callable = Default::default();
        let pipeline = Arc::clone(pipeline);

        Ok(Self {
            buffer,
            _pipeline: pipeline,

            ray_gen,
            hit,
            miss,
            callable,
        })
    }

    fn copy_group_handle(
        pipeline: &RayTracePipeline,
        data: &mut [u8],
        group_index: usize,
        group_size: u32,
    ) -> Result<vk::DeviceAddress, DriverError> {
        let handle = RayTracePipeline::group_handle(pipeline, group_index)?;

        let start = group_index * group_size as usize;
        let end = start + handle.len();

        data[start..end].copy_from_slice(handle);

        Ok(start as _)
    }

    #[allow(unused)]
    pub fn is_valid(&self, pipeline: &Arc<RayTracePipeline>) -> bool {
        Arc::ptr_eq(&self._pipeline, pipeline)
    }

    pub fn regions(
        &self,
    ) -> (
        vk::StridedDeviceAddressRegionKHR,
        vk::StridedDeviceAddressRegionKHR,
        vk::StridedDeviceAddressRegionKHR,
        vk::StridedDeviceAddressRegionKHR,
    ) {
        (self.ray_gen, self.hit, self.miss, self.callable)
    }
}
