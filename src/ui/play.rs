use {
    super::{
        loader::{IdOrKey, LoadInfo, LoadResult, Loader},
        DrawContext, Operation, Ui, UpdateContext,
    },
    crate::{
        art,
        level::{
            nav_mesh::{MeshLocation, NavigationMesh},
            Level,
        },
        render::{
            camera::Camera,
            model::{ModelBuffer, ModelBufferTechnique},
        },
    },
    glam::{vec2, vec3, Mat4, Vec2, Vec3},
    pak::scene::SceneBufGeometry,
    screen_13::prelude::*,
    screen_13_fx::BitmapFont,
    std::sync::Arc,
};

fn read_geometry(geom: &SceneBufGeometry) -> (Vec<u32>, Vec<Vec3>) {
    let transform = Mat4::from_rotation_translation(geom.rotation(), geom.position());
    let indices = geom.index_buf().index_buffer();
    let vertex_data = geom.vertex_data();
    let vertex_count = vertex_data.len() / 12;
    let mut vertices = Vec::with_capacity(vertex_count);

    for idx in 0..vertex_count {
        let vertex = &vertex_data[idx * 12..];
        let x = f32::from_ne_bytes([vertex[0], vertex[1], vertex[2], vertex[3]]);
        let y = f32::from_ne_bytes([vertex[4], vertex[5], vertex[6], vertex[7]]);
        let z = f32::from_ne_bytes([vertex[8], vertex[9], vertex[10], vertex[11]]);
        let vertex = transform.mul_vec4(vec3(x, y, z).extend(1.0)).truncate();

        vertices.push(vertex);
    }

    (indices, vertices)
}

struct Content {
    dare_font: BitmapFont,
}

struct Load {
    loader: Box<dyn Operation<LoadResult>>,
}

impl Operation<Play> for Load {
    fn progress(&self) -> f32 {
        self.loader.progress()
    }

    fn is_done(&self) -> bool {
        self.loader.is_done()
    }

    fn is_err(&self) -> bool {
        self.loader.is_err()
    }

    fn unwrap(self: Box<Self>) -> Play {
        let mut loader = self.loader.unwrap();
        let mut model_buf = loader.model_buf.unwrap();

        let content = Content {
            dare_font: loader
                .fonts
                .remove(art::FONT_KENNEY_MINI_SQUARE_MONO)
                .unwrap(),
        };

        let scene = loader.scenes.remove(art::SCENE_LEVEL_01).unwrap();

        for scene_ref in scene.refs() {
            if let Some(model) = scene_ref.model().map(|id| loader.models[&IdOrKey::Id(id)]) {
                let materials = scene_ref
                    .materials()
                    .iter()
                    .copied()
                    .map(|id| loader.materials[&IdOrKey::Id(id)])
                    .collect::<Box<_>>();
                model_buf.insert_model_instance(
                    model,
                    &materials,
                    scene_ref.position(),
                    scene_ref.rotation(),
                );
            }
        }

        let spawn = scene
            .refs()
            .find(|scene_ref| scene_ref.id() == Some("Spawn"))
            .unwrap();

        let nav_mesh = {
            let walkable_region = scene
                .geometries()
                .find(|geom| geom.id() == Some("Walkable Region"))
                .unwrap();
            let (indices, vertices) = read_geometry(&walkable_region);

            NavigationMesh::new(&indices, &vertices)
        };
        let current_location = nav_mesh.locate(spawn.position());

        let camera = {
            let position = current_location.position() + Play::CAMERA_OFFSET;
            Camera {
                aspect_ratio: 0.0,
                fov_y: 45.0,
                pitch: 0.0,
                yaw: 0.0,
                position,
            }
        };

        let level = Level { nav_mesh };

        Play {
            camera,
            content,
            current_location,
            level,
            model_buf,
        }
    }
}

pub struct Play {
    camera: Camera,
    content: Content,
    current_location: MeshLocation,
    level: Level,
    model_buf: ModelBuffer,
}

impl Play {
    const CAMERA_OFFSET: Vec3 = vec3(0.0, 1.7, 0.0);

    pub fn load(
        device: &Arc<Device>,
        graphics: Option<ModelBufferTechnique>,
    ) -> anyhow::Result<impl Operation<Self>> {
        let loader = Box::new(Loader::spawn_threads(
            device,
            graphics,
            LoadInfo::default()
                .fonts(&[art::FONT_KENNEY_MINI_SQUARE_MONO])
                .scenes(&[art::SCENE_LEVEL_01]),
        )?);

        Ok(Load { loader })
    }

    fn update_camera(&mut self, ui: UpdateContext) {
        let (yaw_delta, pitch_delta) = ui.set_cursor_position_center();

        self.camera.yaw -= yaw_delta * ui.config.mouse_sensitivity;
        self.camera.pitch -= pitch_delta * ui.config.mouse_sensitivity;

        self.camera.yaw %= 360.0;
        self.camera.pitch = self.camera.pitch.clamp(-80.0, 80.0);

        let mut direction = Vec2::ZERO;

        if ui.keyboard.is_down(VirtualKeyCode::W) {
            direction.y += 1.0;
        }

        if ui.keyboard.is_down(VirtualKeyCode::A) {
            direction.x += 1.0;
        }

        if ui.keyboard.is_down(VirtualKeyCode::S) {
            direction.y -= 1.0;
        }

        if ui.keyboard.is_down(VirtualKeyCode::D) {
            direction.x -= 1.0;
        }

        if ui.keyboard.is_down(VirtualKeyCode::LShift) {
            direction.y *= 1.5;
        }

        let yaw = self.camera.yaw - 90f32;
        let yaw = yaw.to_radians();
        let yaw_sin = yaw.sin();
        let yaw_cos = yaw.cos();
        direction = vec2(
            yaw_sin * direction.x - yaw_cos * direction.y,
            yaw_cos * direction.x + yaw_sin * direction.y,
        );

        direction *= ui.dt * 4.0;

        self.current_location = self.level.nav_mesh.walk(self.current_location, direction);
        self.camera.position = self.current_location.position() + Self::CAMERA_OFFSET;
    }
}

impl Ui for Play {
    fn draw(&mut self, frame: DrawContext) {
        let framebuffer_info = frame.render_graph.node_info(frame.framebuffer_image);

        self.camera.aspect_ratio = framebuffer_info.width as f32 / framebuffer_info.height as f32;

        // TODO: Remove before flight
        frame
            .render_graph
            .clear_color_image_value(frame.framebuffer_image, [0xFF, 0x00, 0xFF, 0xFF]);

        self.model_buf
            .record(
                frame.render_graph,
                frame.framebuffer_image,
                &mut self.camera,
                // &self.sun,
            )
            .unwrap();

        self.content.dare_font.print(
            frame.render_graph,
            frame.framebuffer_image,
            0.0,
            0.0,
            [0xff, 0xff, 0xff],
            format!("FPS: {}", (1.0 / frame.dt).round()),
        );
    }

    fn update(mut self: Box<Self>, ui: UpdateContext) -> Option<Box<dyn Ui>> {
        #[cfg(debug_assertions)]
        if ui.keyboard.is_pressed(&VirtualKeyCode::Escape) {
            return None;
        }

        self.update_camera(ui);

        Some(self)
    }
}
