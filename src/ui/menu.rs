use {
    super::{
        loader::{LoadInfo, LoadResult, Loader},
        play::Play,
        transition::{Transition, TransitionInfo},
        CursorStyle, DrawContext, Operation, Ui, UpdateContext,
    },
    crate::{
        art,
        render::bitmap::{Bitmap, BitmapBuffer, Rect},
    },
    kira::sound::static_sound::StaticSoundData,
    screen_13::prelude::*,
    screen_13_fx::BitmapFont,
    std::{cell::RefCell, sync::Arc, time::Duration},
};

struct Button {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    text: &'static str,
    text_layout: ([i32; 2], [u32; 2]),
    is_pressed: bool,
}

struct Content {
    blue_button_bottom: Bitmap,
    blue_button_bottom_corner: Bitmap,
    blue_button_middle: Bitmap,
    blue_button_side: Bitmap,
    blue_button_top_corner: Bitmap,
    blue_button_top: Bitmap,

    beep_sound: StaticSoundData,
    small_font: BitmapFont,
}

impl Content {
    fn draw_blue_button(
        &self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        bitmaps: &mut Vec<(Bitmap, Rect)>,
    ) {
        Self::draw_six_slice(
            self.blue_button_top_corner,
            self.blue_button_top,
            self.blue_button_side,
            self.blue_button_bottom_corner,
            self.blue_button_bottom,
            self.blue_button_middle,
            x,
            y,
            width,
            height,
            bitmaps,
        );
    }

    fn draw_six_slice(
        top_corner: Bitmap,
        top: Bitmap,
        side: Bitmap,
        bottom_corner: Bitmap,
        bottom: Bitmap,
        middle: Bitmap,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        bitmaps: &mut Vec<(Bitmap, Rect)>,
    ) {
        let (top_corner_width, top_corner_height) = top_corner.size();
        let (_, top_height) = top.size();
        let (side_width, _) = side.size();
        let (bottom_corner_width, bottom_corner_height) = bottom_corner.size();

        // Top left
        bitmaps.push((
            top_corner,
            Rect::new(x, y, top_corner_width as _, top_corner_height as _),
        ));

        bitmaps.push((
            top,
            Rect::new(
                x + top_corner_width as i32,
                y,
                width as i32 - (2 * (top_corner_width as i32)),
                top_height as i32,
            ),
        ));

        // Top right
        bitmaps.push((
            top_corner,
            Rect::new(
                x + width as i32,
                y,
                -(top_corner_width as i32),
                top_corner_height as _,
            ),
        ));

        // Left
        bitmaps.push((
            side,
            Rect::new(
                x,
                y + top_corner_height as i32,
                side_width as _,
                height as i32 - (top_corner_height as i32 + bottom_corner_height as i32),
            ),
        ));

        // Right
        bitmaps.push((
            side,
            Rect::new(
                x + width as i32,
                y + top_corner_height as i32,
                -(side_width as i32),
                height as i32 - (top_corner_height as i32 + bottom_corner_height as i32),
            ),
        ));

        // Bottom left
        bitmaps.push((
            bottom_corner,
            Rect::new(
                x,
                y + height as i32 - bottom_corner_height as i32,
                bottom_corner_width as _,
                bottom_corner_height as _,
            ),
        ));

        bitmaps.push((
            bottom,
            Rect::new(
                x + bottom_corner_width as i32,
                y + height as i32 - bottom_corner_height as i32,
                width as i32 - (2 * (bottom_corner_width as i32)),
                bottom_corner_height as _,
            ),
        ));

        // Bottom right
        bitmaps.push((
            bottom_corner,
            Rect::new(
                x + width as i32,
                y + height as i32 - bottom_corner_height as i32,
                -(bottom_corner_width as i32),
                bottom_corner_height as _,
            ),
        ));

        bitmaps.push((
            middle,
            Rect::new(
                x + side_width as i32,
                y + top_height as i32,
                width as i32 - 2 * (side_width as i32),
                height as i32 - (top_height as i32 + bottom_corner_height as i32),
            ),
        ));
    }
}

struct Gui {
    play_button: Button,
    valid_framebuffer: (u32, u32),
}

impl Gui {
    fn is_valid(&self, framebuffer_width: u32, framebuffer_height: u32) -> bool {
        self.valid_framebuffer == (framebuffer_width, framebuffer_height)
    }

    fn layout(&mut self, content: &Content, framebuffer_width: u32, framebuffer_height: u32) {
        if self.is_valid(framebuffer_width, framebuffer_height) {
            return;
        }

        self.play_button.text_layout = content.small_font.measure(&self.play_button.text);
        self.play_button.width = self.play_button.text_layout.1[0] + 10;
        self.play_button.height = self.play_button.text_layout.1[1] + 8;
        self.play_button.x = framebuffer_width as i32 / 2 - self.play_button.width as i32 / 2;
        self.play_button.y = framebuffer_height as i32 / 2 - self.play_button.height as i32 / 2;

        self.valid_framebuffer = (framebuffer_width, framebuffer_height);
    }
}

struct Load {
    device: Arc<Device>,
    loader: Box<dyn Operation<LoadResult>>,
}

impl Operation<Menu> for Load {
    fn progress(&self) -> f32 {
        self.loader.progress()
    }

    fn is_done(&self) -> bool {
        self.loader.is_done()
    }

    fn is_err(&self) -> bool {
        self.loader.is_err()
    }

    fn unwrap(self: Box<Self>) -> Menu {
        let device = Arc::clone(&self.device);
        let mut loader = self.loader.unwrap();
        let bitmap_buf = loader.bitmap_buf.unwrap();

        let content = Content {
            blue_button_bottom: loader
                .bitmaps
                .remove(art::BITMAP_BLUE_BUTTON_BOTTOM_PNG)
                .unwrap(),
            blue_button_bottom_corner: loader
                .bitmaps
                .remove(art::BITMAP_BLUE_BUTTON_BOTTOM_CORNER_PNG)
                .unwrap(),
            blue_button_middle: loader
                .bitmaps
                .remove(art::BITMAP_BLUE_BUTTON_MIDDLE_PNG)
                .unwrap(),
            blue_button_side: loader
                .bitmaps
                .remove(art::BITMAP_BLUE_BUTTON_SIDE_PNG)
                .unwrap(),
            blue_button_top: loader
                .bitmaps
                .remove(art::BITMAP_BLUE_BUTTON_TOP_PNG)
                .unwrap(),
            blue_button_top_corner: loader
                .bitmaps
                .remove(art::BITMAP_BLUE_BUTTON_TOP_CORNER_PNG)
                .unwrap(),

            beep_sound: loader
                .sounds
                .remove(art::SOUND_DIGITAL_THREE_TONE_1_OGG)
                .unwrap(),
            small_font: loader
                .fonts
                .remove(art::FONT_KENNEY_MINI_SQUARE_MONO)
                .unwrap(),
        };

        Menu {
            bitmap_buf,
            content,
            device,
            gui: Gui {
                play_button: Button {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                    text: "Press any key to continue",
                    text_layout: ([0, 0], [0, 0]),
                    is_pressed: false,
                },
                valid_framebuffer: (0, 0),
            },
            play: None,
        }
    }
}

pub struct Menu {
    bitmap_buf: BitmapBuffer,
    content: Content,
    device: Arc<Device>,
    gui: Gui,
    play: Option<Box<dyn Operation<Play>>>,
}

impl Menu {
    pub fn load(device: &Arc<Device>) -> anyhow::Result<impl Operation<Self>> {
        let device = Arc::clone(device);
        let loader = Box::new(Loader::spawn_threads(
            &device,
            None,
            LoadInfo::default()
                .bitmaps(&[
                    art::BITMAP_BLUE_BUTTON_BOTTOM_PNG,
                    art::BITMAP_BLUE_BUTTON_BOTTOM_CORNER_PNG,
                    art::BITMAP_BLUE_BUTTON_MIDDLE_PNG,
                    art::BITMAP_BLUE_BUTTON_SIDE_PNG,
                    art::BITMAP_BLUE_BUTTON_TOP_PNG,
                    art::BITMAP_BLUE_BUTTON_TOP_CORNER_PNG,
                ])
                .fonts(&[art::FONT_KENNEY_MINI_SQUARE_MONO])
                .sounds(&[art::SOUND_DIGITAL_THREE_TONE_1_OGG]),
        )?);

        Ok(Load { device, loader })
    }
}

impl Ui for Menu {
    fn draw(&mut self, frame: DrawContext) {
        frame
            .render_graph
            .clear_color_image_value(frame.framebuffer_image, [0.25, 0.0, 0.25, 1.0]);

        thread_local! {
            static BITMAPS: RefCell<Vec<(Bitmap, Rect)>> = Default::default();
        }

        let framebuffer_info = frame.render_graph.node_info(frame.framebuffer_image);

        self.gui.layout(
            &self.content,
            framebuffer_info.width,
            framebuffer_info.height,
        );

        BITMAPS.with(|bitmaps| {
            let mut bitmaps = bitmaps.borrow_mut();
            bitmaps.clear();
            self.content.draw_blue_button(
                self.gui.play_button.x,
                self.gui.play_button.y,
                self.gui.play_button.width,
                self.gui.play_button.height as _,
                &mut bitmaps,
            );

            self.bitmap_buf
                .record(
                    frame.render_graph,
                    frame.framebuffer_image,
                    bitmaps.as_slice(),
                )
                .unwrap();
        });

        self.content.small_font.print(
            frame.render_graph,
            frame.framebuffer_image,
            (self.gui.play_button.x + (self.gui.play_button.width as i32 / 2)
                - (self.gui.play_button.text_layout.1[0] as i32 / 2)) as _,
            (self.gui.play_button.y + (self.gui.play_button.height as i32 / 2)
                - (self.gui.play_button.text_layout.1[1] as i32 / 2)
                - 3) as _,
            [0x00, 0x00, 0x00],
            self.gui.play_button.text,
        );

        self.content.small_font.print(
            frame.render_graph,
            frame.framebuffer_image,
            0.0,
            0.0,
            [0xff, 0xff, 0xff],
            format!("FPS: {}", (1.0 / frame.dt).round()),
        );
    }

    fn update(mut self: Box<Self>, ui: UpdateContext) -> Option<Box<dyn Ui>> {
        *ui.cursor = Some(CursorStyle::PointerShadow);

        #[cfg(debug_assertions)]
        if ui.keyboard.is_pressed(&VirtualKeyCode::Escape) {
            return None;
        }

        if self.play.is_none() {
            self.play = Some(Box::new(
                Play::load(&self.device, ui.config.graphics).unwrap(),
            ));
        }

        if let Some(play) = &self.play {
            if play.is_err() {
                panic!();
            }

            if play.is_done() {
                if self
                    .gui
                    .is_valid(ui.framebuffer_width, ui.framebuffer_height)
                {
                    if true || ui.mouse.is_pressed(MouseButton::Left) {
                        let (mouse_x, mouse_y) = ui.mouse.position();
                        let mouse_x = (mouse_x / ui.framebuffer_scale) as i32;
                        let mouse_y = (mouse_y / ui.framebuffer_scale) as i32;

                        if true
                            || mouse_x >= self.gui.play_button.x
                                && mouse_y >= self.gui.play_button.y
                                && mouse_x
                                    <= self.gui.play_button.x + self.gui.play_button.width as i32
                                && mouse_y
                                    <= self.gui.play_button.y + self.gui.play_button.height as i32
                        {
                            let play = Box::new(self.play.take().unwrap().unwrap());

                            *ui.cursor = None;

                            #[cfg(not(debug_assertions))]
                            ui.window
                                .set_cursor_grab(CursorGrabMode::Confined)
                                .unwrap_or_default();

                            ui.set_cursor_position_center();

                            return Some(Box::new(Transition::new(
                                self,
                                play,
                                TransitionInfo::Fade,
                                Duration::from_secs_f32(0.25),
                            )));
                        }
                    }
                }
            }
        }

        Some(self)
    }
}
