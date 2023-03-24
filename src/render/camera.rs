#![allow(unused)]

use {
    glam::{Mat4, Vec3},
    std::{cell::Cell, ops::Range},
};

pub struct Camera {
    pub aspect_ratio: f32,
    pub fov_y: f32,
    pub pitch: f32,
    pub yaw: f32,
    pub position: Vec3,
}
