use {
    super::{
        loader::{LoadInfo, LoadResult, Loader},
        menu::Menu,
        transition::{Transition, TransitionInfo},
        DrawContext, Operation, Ui, UpdateContext,
    },
    crate::art,
    kira::sound::static_sound::StaticSoundData,
    screen_13::prelude::*,
    screen_13_fx::BitmapFont,
    std::{
        sync::Arc,
        time::{Duration, Instant},
    },
};

struct Content {
    beep_sound: StaticSoundData,
    small_font: BitmapFont,
}

struct Load {
    device: Arc<Device>,
    loader: Box<dyn Operation<LoadResult>>,
}

impl Operation<Title> for Load {
    fn progress(&self) -> f32 {
        self.loader.progress()
    }

    fn is_done(&self) -> bool {
        self.loader.is_done()
    }

    fn is_err(&self) -> bool {
        self.loader.is_err()
    }

    fn unwrap(self: Box<Self>) -> Title {
        let device = Arc::clone(&self.device);
        let mut loader = self.loader.unwrap();

        let content = Content {
            beep_sound: loader
                .sounds
                .remove(art::SOUND_DIGITAL_THREE_TONE_1_OGG)
                .unwrap(),
            small_font: loader
                .fonts
                .remove(art::FONT_KENNEY_MINI_SQUARE_MONO)
                .unwrap(),
        };

        Title {
            beeped: false,
            content,
            device,
            menu: None,
            skip_requested: false,
            started: Instant::now(),
        }
    }
}

pub struct Title {
    beeped: bool,
    content: Content,
    device: Arc<Device>,
    menu: Option<Box<dyn Operation<Menu>>>,
    skip_requested: bool,
    started: Instant,
}

impl Title {
    pub fn load(device: &Arc<Device>) -> anyhow::Result<impl Operation<Self>> {
        let device = Arc::clone(device);
        let loader = Box::new(Loader::spawn_threads(
            &device,
            None,
            LoadInfo::default()
                .fonts(&[art::FONT_KENNEY_MINI_SQUARE_MONO])
                .sounds(&[art::SOUND_DIGITAL_THREE_TONE_1_OGG]),
        )?);

        Ok(Load { device, loader })
    }
}

impl Ui for Title {
    fn draw(&mut self, frame: DrawContext) {
        frame
            .render_graph
            .clear_color_image(frame.framebuffer_image);

        let framebuffer_info = frame.render_graph.node_info(frame.framebuffer_image);

        {
            let text = "Mood";
            let ([x, y], [width, height]) = self.content.small_font.measure(text);
            self.content.small_font.print(
                frame.render_graph,
                frame.framebuffer_image,
                (framebuffer_info.width as i32 / 2 - width as i32 / 2 + x / 2) as _,
                (framebuffer_info.height as i32 / 2 - height as i32 / 2 + y / 2) as _,
                [0xcc, 0xcc, 0xcc],
                text,
            );
        }

        {
            let text = "copyright 2023 john wells";
            let ([x, y], [width, height]) = self.content.small_font.measure(text);
            self.content.small_font.print(
                frame.render_graph,
                frame.framebuffer_image,
                (framebuffer_info.width as i32 / 2 - width as i32 / 2 + x / 2) as _,
                (framebuffer_info.height as i32 - height as i32 + y / 2) as _,
                [0xcc, 0xcc, 0xcc],
                text,
            );
        }
    }

    fn update(mut self: Box<Self>, ui: UpdateContext) -> Option<Box<dyn Ui>> {
        #[cfg(debug_assertions)]
        if ui.keyboard.is_pressed(&VirtualKeyCode::Escape) {
            return None;
        }

        if ui.keyboard.any_pressed() {
            self.skip_requested = true;
        }

        if self.menu.is_none() {
            self.menu = Some(Box::new(Menu::load(&self.device).unwrap()));
        }

        let elapsed = (Instant::now() - self.started).as_secs_f32();

        if !self.beeped {
            self.beeped = true;

            if let Some(audio) = ui.audio {
                audio.play(self.content.beep_sound.clone()).unwrap();
            }
        }

        #[cfg(debug_assertions)]
        let until_skip = 0.5;

        #[cfg(not(debug_assertions))]
        let until_skip = 4.0;

        if elapsed > until_skip {
            self.skip_requested = true;
        }

        if self.skip_requested {
            if let Some(menu) = &self.menu {
                if menu.is_err() {
                    panic!("Unable to load menu");
                }

                if menu.is_done() {
                    let menu = Box::new(self.menu.take().unwrap().unwrap());

                    #[cfg(debug_assertions)]
                    let duration = 0.1;

                    #[cfg(not(debug_assertions))]
                    let duration = 0.25;

                    return Some(Box::new(Transition::new(
                        self,
                        menu,
                        TransitionInfo::Fade,
                        Duration::from_secs_f32(duration),
                    )));
                }
            }
        }

        Some(self)
    }
}
