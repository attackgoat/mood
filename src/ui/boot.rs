use {
    super::{
        title::Title,
        transition::{Transition, TransitionInfo},
        DrawContext, Operation, Ui, UpdateContext,
    },
    screen_13::prelude::*,
    std::{sync::Arc, time::Duration},
};

pub struct Boot {
    device: Arc<Device>,
    loader: Option<Box<dyn Operation<Title>>>,
}

impl Boot {
    pub fn new(device: &Arc<Device>) -> Self {
        let device = Arc::clone(device);

        Self {
            device,
            loader: None,
        }
    }
}

impl Ui for Boot {
    fn draw(&mut self, frame: DrawContext) {
        frame
            .render_graph
            .clear_color_image(frame.framebuffer_image);
    }

    fn update(mut self: Box<Self>, ui: UpdateContext) -> Option<Box<dyn Ui>> {
        if let Some(loader) = &self.loader {
            if loader.is_err() {
                panic!();
            }

            if loader.is_done() {
                let title = Box::new(self.loader.take().unwrap().unwrap());

                #[cfg(debug_assertions)]
                let duration = 0.25;

                #[cfg(not(debug_assertions))]
                let duration = 1.0;

                return Some(Box::new(Transition::new(
                    self,
                    title,
                    TransitionInfo::Fade,
                    Duration::from_secs_f32(duration),
                )));
            }
        } else {
            ui.window.set_cursor_visible(false);

            self.loader = Some(Box::new(Title::load(&self.device).unwrap()));
        }

        Some(self)
    }
}
