// This file should be replaced with a library, but not sure which one to use yet
// Used stuff from here: https://github.com/rustgd/collision-rs/blob/master/src/

#![allow(unused)]

use glam::{vec4, Vec3, Vec4};

pub const fn align_up_u32(val: u32, atom: u32) -> u32 {
    (val + atom - 1) & !(atom - 1)
}

pub const fn align_up_u64(val: u64, atom: u64) -> u64 {
    (val + atom - 1) & !(atom - 1)
}

#[derive(Clone, Copy, Debug)]
pub struct Plane {
    normal: Vec3,
    distance: f32,
}

impl Plane {
    pub fn from_position_normal(position: Vec3, normal: Vec3) -> Self {
        debug_assert!(normal.is_normalized());

        Self {
            normal,
            distance: position.dot(normal),
        }
    }

    pub fn intersect_ray(self, ray: Ray) -> Option<Vec3> {
        let t = -(self.distance + ray.position.dot(self.normal)) / ray.normal.dot(self.normal);

        if t >= 0.0 {
            Some(ray.position + ray.normal * t)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Ray {
    position: Vec3,
    normal: Vec3,
}

impl Ray {
    pub fn new(position: Vec3, normal: Vec3) -> Self {
        debug_assert!(normal.is_normalized());

        Self { position, normal }
    }

    pub fn intersect_plane(self, plane: Plane) -> Option<Vec3> {
        plane.intersect_ray(self)
    }
}
