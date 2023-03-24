use {
    super::{
        loader::{IdOrKey, LoadInfo, LoadResult, Loader},
        transition::{Transition, TransitionInfo},
        CursorStyle, DrawContext, Operation, Ui, UpdateContext,
    },
    crate::{
        art,
        math::{Plane, Ray},
        render::{
            camera::Camera,
            model::{Material, Model, ModelBuffer},
        },
    },
    glam::{vec2, vec3, Vec3},
    pak::scene::SceneBuf,
    screen_13::prelude::*,
    screen_13_fx::BitmapFont,
    std::{
        sync::Arc,
        time::{Duration, Instant},
    },
};

struct Boot {
    device: Arc<Device>,
    step: Option<BootStep>,
}

impl Ui for Boot {
    fn draw(&mut self, frame: DrawContext) {
        frame
            .render_graph
            .clear_color_image(frame.framebuffer_image);

        if let Some(BootStep::LoadBench { font, loader }) = &mut self.step {
            let progress = (loader.progress() * 100.0) as u8;
            let text = format!("Loading {progress}%...");
            let ([x, y], [width, height]) = font.measure(&text);
            let framebuffer_info = frame.render_graph.node_info(frame.framebuffer_image);
            let x = framebuffer_info.width as i32 / 2 - width as i32 / 2 + x / 2;
            let y = framebuffer_info.height as i32 / 2 - height as i32 / 2 + y / 2;
            let color = [0xff, 0xff, 0xff];

            font.print(
                frame.render_graph,
                frame.framebuffer_image,
                x as f32,
                y as f32,
                color,
                text,
            );
        }
    }

    fn update(mut self: Box<Self>, ui: UpdateContext) -> Option<Box<dyn Ui>> {
        match self.step.take() {
            None => {
                let loader = Box::new(
                    Loader::spawn_threads(
                        &self.device,
                        ui.config.graphics,
                        LoadInfo::default().fonts(&[art::FONT_KENNEY_MINI_SQUARE_MONO]),
                    )
                    .unwrap(),
                );
                self.step = Some(BootStep::LoadFont { loader });
            }
            Some(BootStep::LoadFont { loader }) => {
                if loader.is_err() {
                    panic!();
                } else if loader.is_done() {
                    let mut loader = loader.unwrap();
                    let font = loader
                        .fonts
                        .remove(art::FONT_KENNEY_MINI_SQUARE_MONO)
                        .unwrap();
                    let loader = Box::new(
                        Loader::spawn_threads(
                            &self.device,
                            ui.config.graphics,
                            LoadInfo::default()
                                .fonts(&[art::FONT_KENNEY_MINI_SQUARE_MONO])
                                .scenes(&[art::SCENE_LEVEL_01]),
                        )
                        .unwrap(),
                    );
                    self.step = Some(BootStep::LoadBench { font, loader });
                } else {
                    self.step = Some(BootStep::LoadFont { loader });
                }
            }
            Some(BootStep::LoadBench { font, loader }) => {
                if loader.is_err() {
                    panic!();
                } else if loader.is_done() {
                    let device = Arc::clone(&self.device);
                    let mut loader = loader.unwrap();
                    let mut model_buf = loader.model_buf.unwrap();

                    let content = Content {
                        dare_font: loader
                            .fonts
                            .remove(art::FONT_KENNEY_MINI_SQUARE_MONO)
                            .unwrap(),
                        level: loader.scenes.remove(art::SCENE_LEVEL_01).unwrap(),
                    };

                    for scene_ref in content.level.refs() {
                        if let Some(model) =
                            scene_ref.model().map(|id| loader.models[&IdOrKey::Id(id)])
                        {
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

                    let camera = {
                        let position = Vec3::new(40.0, 11.0, 0.0);
                        Camera {
                            aspect_ratio: 0.0,
                            fov_y: 45.0,
                            pitch: 0.0,
                            yaw: 0.0,
                            position,
                        }
                    };

                    let bench = Bench {
                        camera,
                        content,
                        device,
                        frame_index: 0,
                        model_buf,
                        time_started: Instant::now(),
                    };

                    return Some(Box::new(bench));
                } else {
                    self.step = Some(BootStep::LoadBench { font, loader });
                }
            }
        }

        Some(self)
    }
}

enum BootStep {
    LoadFont {
        loader: Box<Loader>,
    },
    LoadBench {
        font: BitmapFont,
        loader: Box<Loader>,
    },
}

struct Content {
    dare_font: BitmapFont,
    level: SceneBuf,
}

pub struct Bench {
    camera: Camera,
    content: Content,
    device: Arc<Device>,
    frame_index: usize,
    model_buf: ModelBuffer,
    // pool: LazyPool,
    time_started: Instant,
}

impl Bench {
    const FRAME_COUNT: usize = 1000;

    pub fn boot(device: &Arc<Device>) -> impl Ui {
        let device = Arc::clone(device);

        Boot { device, step: None }
    }
}

impl Ui for Bench {
    fn draw(&mut self, frame: DrawContext) {
        let framebuffer_info = frame.render_graph.node_info(frame.framebuffer_image);

        self.camera.aspect_ratio = framebuffer_info.width as f32 / framebuffer_info.height as f32;

        self.model_buf
            .record(
                frame.render_graph,
                frame.framebuffer_image,
                &mut self.camera,
                // &self.sun,
            )
            .unwrap();

        self.frame_index += 1;
    }

    fn update(self: Box<Self>, ui: UpdateContext) -> Option<Box<dyn Ui>> {
        if self.frame_index == Self::FRAME_COUNT {
            let frames_per_sec = Self::FRAME_COUNT * 1_000
                / Instant::now().duration_since(self.time_started).as_millis() as usize;

            Some(Box::new(BenchResult {
                font: self.content.dare_font,
                frames_per_sec,
            }))
        } else if ui.keyboard.any_pressed() {
            None
        } else {
            Some(self)
        }
    }
}

pub struct BenchResult {
    font: BitmapFont,
    frames_per_sec: usize,
}

impl Ui for BenchResult {
    fn draw(&mut self, frame: DrawContext) {
        frame
            .render_graph
            .clear_color_image(frame.framebuffer_image);

        let text = format!("{} FPS", self.frames_per_sec);
        let ([x, y], [width, height]) = self.font.measure(&text);
        let framebuffer_info = frame.render_graph.node_info(frame.framebuffer_image);
        let x = framebuffer_info.width as i32 / 2 - width as i32 / 2 + x / 2;
        let y = framebuffer_info.height as i32 / 2 - height as i32 / 2 + y / 2;
        let color = [0xff, 0xff, 0xff];

        self.font.print(
            frame.render_graph,
            frame.framebuffer_image,
            x as f32,
            y as f32,
            color,
            text,
        );
    }

    fn update(self: Box<Self>, ui: UpdateContext) -> Option<Box<dyn Ui>> {
        if ui.keyboard.any_pressed() {
            None
        } else {
            Some(self)
        }
    }
}
