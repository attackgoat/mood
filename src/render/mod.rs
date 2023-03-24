pub mod bitmap;
pub mod camera;
pub mod model;

mod bounding_sphere;
mod excl_sum;

use {
    crate::res,
    bytemuck::{bytes_of, cast_slice, NoUninit},
    pak::{Pak, PakBuf},
    screen_13::prelude::*,
};

#[cfg(feature = "hot-shaders")]
use std::path::PathBuf;

fn lease_buffer(
    pool: &mut impl Pool<BufferInfoBuilder, Buffer>,
    data: &[u8],
    usage: vk::BufferUsageFlags,
) -> Result<Lease<Buffer>, DriverError> {
    let mut buf = pool.lease(BufferInfo::new_mappable(data.len() as _, usage))?;
    Buffer::copy_from_slice(&mut buf, 0, data);

    Ok(buf)
}

fn lease_storage_buffer<T>(
    pool: &mut impl Pool<BufferInfoBuilder, Buffer>,
    data: &[T],
) -> Result<Lease<Buffer>, DriverError>
where
    T: NoUninit,
{
    let data = cast_slice(data);

    lease_buffer(pool, data, vk::BufferUsageFlags::STORAGE_BUFFER)
}

fn lease_uniform_buffer<T>(
    pool: &mut impl Pool<BufferInfoBuilder, Buffer>,
    data: T,
) -> Result<Lease<Buffer>, DriverError>
where
    T: NoUninit,
{
    let data = bytes_of(&data);

    lease_buffer(pool, data, vk::BufferUsageFlags::UNIFORM_BUFFER)
}

fn open_res_pak() -> Result<PakBuf, DriverError> {
    res::open_pak().map_err(|err| {
        error!("Unable to open resource file: {err}");

        DriverError::InvalidData
    })
}

fn read_blob(pak: &mut PakBuf, key: &str) -> Result<Vec<u8>, DriverError> {
    pak.read_blob(key).map_err(|err| {
        error!("Unable to read blob {key}: {err}");

        DriverError::InvalidData
    })
}

#[cfg(feature = "hot-shaders")]
fn res_shader_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("res/shader")
}

#[cfg(test)]
mod tests {
    #[cfg_attr(target_os = "macos", test)]
    pub fn run_tests() {
        super::bounding_sphere::tests::bounding_sphere1();
        super::bounding_sphere::tests::bounding_sphere2();
        super::bounding_sphere::tests::bounding_sphere3();

        super::excl_sum::tests::exclusive_sum1();
        super::excl_sum::tests::exclusive_sum2();
        super::excl_sum::tests::exclusive_sum3();
        super::excl_sum::tests::exclusive_sum4();
    }
}
