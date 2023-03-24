use {
    super::Config,
    kira::manager::{backend::cpal::CpalBackend, AudioManager},
    screen_13::prelude::*,
    screen_13_fx::TransitionPipeline,
};

pub mod bench;
pub mod boot;

mod loader;
mod menu;
mod play;
mod title;
mod transition;

#[derive(Clone, Copy)]
pub enum CursorStyle {
    Pointer,
    PointerShadow,
}

pub struct DrawContext<'a> {
    pub dt: f32,
    pub framebuffer_image: ImageLeaseNode,
    pub pool: &'a mut LazyPool,
    pub render_graph: &'a mut RenderGraph,
    pub transition_pipeline: &'a mut TransitionPipeline,
}

pub trait Operation<T> {
    fn progress(&self) -> f32;
    fn is_done(&self) -> bool;
    fn is_err(&self) -> bool;
    fn unwrap(self: Box<Self>) -> T;
}

pub trait Ui {
    fn draw(&mut self, frame: DrawContext);

    fn update(self: Box<Self>, ui: UpdateContext) -> Option<Box<dyn Ui>>;
}

pub struct UpdateContext<'a> {
    pub audio: Option<&'a mut AudioManager<CpalBackend>>,
    pub config: &'a Config,
    pub cursor: &'a mut Option<CursorStyle>,
    pub dt: f32,
    pub events: &'a [Event<'a, ()>],
    pub framebuffer_aspect_ratio: f32,
    pub framebuffer_height: u32,
    pub framebuffer_scale: f32,
    pub framebuffer_width: u32,
    pub keyboard: &'a KeyBuf,
    pub mouse: &'a MouseBuf,
    pub window: &'a Window,
}

impl<'a> UpdateContext<'a> {
    fn set_cursor_position_center(&self) -> (f32, f32) {
        if !self.window.has_focus() {
            return (0.0, 0.0);
        }

        let size = self.window.inner_size();
        let center = PhysicalPosition::new(size.width >> 1, size.height >> 1);
        self.window.set_cursor_position(center).unwrap_or_default();

        let (x, y) = self.mouse.position();

        (x / size.width as f32 - 0.5, y / size.height as f32 - 0.5)
    }
}
