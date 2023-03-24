pub use screen_13_fx::Transition as TransitionInfo;

use {
    super::{DrawContext, Ui, UpdateContext},
    screen_13::prelude::*,
    std::time::{Duration, Instant},
};

pub struct Transition {
    a: Box<dyn Ui>,
    b: Box<dyn Ui>,
    duration_secs: f32,
    info: TransitionInfo,
    progress: f32,
    started_at: Instant,
}

impl Transition {
    pub fn new(a: Box<dyn Ui>, b: Box<dyn Ui>, info: TransitionInfo, duration: Duration) -> Self {
        let started_at = Instant::now();
        let progress = 0.0;
        let duration_secs = duration.as_secs_f32();

        Self {
            a,
            b,
            duration_secs,
            info,
            progress,
            started_at,
        }
    }
}

impl Ui for Transition {
    fn draw(&mut self, frame: DrawContext) {
        let framebuffer_info = frame.render_graph.node_info(frame.framebuffer_image);
        let a_framebuffer = frame
            .render_graph
            .bind_node(frame.pool.lease(framebuffer_info).unwrap());
        let b_framebuffer = frame
            .render_graph
            .bind_node(frame.pool.lease(framebuffer_info).unwrap());

        self.a.draw(DrawContext {
            dt: frame.dt,
            framebuffer_image: a_framebuffer,
            pool: frame.pool,
            render_graph: frame.render_graph,
            transition_pipeline: frame.transition_pipeline,
        });
        self.b.draw(DrawContext {
            dt: frame.dt,
            framebuffer_image: b_framebuffer,
            pool: frame.pool,
            render_graph: frame.render_graph,
            transition_pipeline: frame.transition_pipeline,
        });

        self.progress = (Instant::now() - self.started_at).as_secs_f32() / self.duration_secs;

        frame.transition_pipeline.apply_to(
            frame.render_graph,
            a_framebuffer,
            b_framebuffer,
            frame.framebuffer_image,
            self.info,
            self.progress,
        );
    }

    fn update(self: Box<Self>, _: UpdateContext) -> Option<Box<dyn Ui>> {
        Some(if self.progress >= 1.0 { self.b } else { self })
    }
}
