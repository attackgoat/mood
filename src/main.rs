mod art {
    include!(concat!(env!("OUT_DIR"), "/art.rs"));

    use {super::env::current_exe_dir, pak::PakBuf, std::io::Error};

    pub fn open_pak() -> Result<PakBuf, Error> {
        let path = current_exe_dir().join("art.pak");

        PakBuf::open(path)
    }
}

mod res {
    include!(concat!(env!("OUT_DIR"), "/res.rs"));

    use {super::env::current_exe_dir, pak::PakBuf, std::io::Error};

    pub fn open_pak() -> Result<PakBuf, Error> {
        let path = current_exe_dir().join("res.pak");

        PakBuf::open(path)
    }
}

mod fs {
    use directories::ProjectDirs;

    pub const APPLICATION: &str = "Mood";
    pub const ORGANIZATION: &str = "Attack Goat";
    pub const QUALIFIER: &str = "com";

    pub fn project_dirs() -> Option<ProjectDirs> {
        ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
    }
}

mod args;
mod config;
mod env;
mod level;
mod math;
mod render;
mod ui;

use {
    self::{
        args::Args,
        config::Config,
        ui::{bench::Bench, boot::Boot, CursorStyle, DrawContext, Ui, UpdateContext},
    },
    anyhow::Context,
    bytemuck::{bytes_of, cast_slice},
    clap::Parser,
    glam::{vec3, vec4, Mat4},
    kira::manager::{backend::cpal::CpalBackend, AudioManager, AudioManagerSettings},
    pak::{bitmap::BitmapFormat, Pak, PakBuf},
    screen_13::prelude::*,
    screen_13_fx::{ImageFormat, ImageLoader, TransitionPipeline},
    std::{
        panic::{set_hook, take_hook},
        process::exit,
        sync::Arc,
        time::Instant,
    },
};

fn main() {
    #[cfg(debug_assertions)]
    pretty_env_logger::init();

    set_thread_panic_hook();

    let args = Args::parse();
    let config = Config::read();

    let mut event_loop = EventLoop::new();

    #[cfg(debug_assertions)]
    if args.debug_vulkan {
        event_loop = event_loop.debug(true);
    }

    if args.window {
        if let Some(monitor) = event_loop
            .primary_monitor()
            .or_else(|| event_loop.available_monitors().next())
        {
            // If the --window argument is provided we render in windowed mode where the window is
            // three quarters of the total screen size and centered in the screen
            let monitor_size = monitor.size();
            let window_size =
                PhysicalSize::new(monitor_size.width * 3 / 4, monitor_size.height * 3 / 4);
            let window_position = PhysicalPosition::new(
                monitor_size.width / 2 - window_size.width / 2,
                monitor_size.height / 2 - window_size.height / 2,
            );
            event_loop = event_loop.window(|window| {
                window
                    .with_inner_size(window_size)
                    .with_position(window_position)
            });
        } else {
            // In the unlikely event we are not able to find the montior details we just wing it
            event_loop =
                event_loop.window(|window| window.with_inner_size(PhysicalSize::new(1280, 720)));
        }
    } else {
        event_loop = event_loop.fullscreen_mode(FullscreenMode::Exclusive);
    }

    let not_mute = !args.mute;
    let mut audio = not_mute.then(|| {
        AudioManager::<CpalBackend>::new(AudioManagerSettings::default())
            .context("Creating audio")
            .unwrap()
    });

    let mut res_pak = res::open_pak().unwrap();
    let window_icon = read_icon(res::ICON_WINDOW, &mut res_pak);

    let event_loop = event_loop
        .window(|window| {
            window
                .with_title(fs::APPLICATION)
                .with_window_icon(Some(window_icon))
        })
        .sync_display(config.v_sync)
        .build()
        .unwrap();

    let mut pool = LazyPool::new(&event_loop.device);

    trace!("Starting");

    let mut image_loader = ImageLoader::new(&event_loop.device).unwrap();
    let cursor_pointer = read_cursor(res::CURSOR_POINTER_PNG, &mut res_pak, &mut image_loader);
    let cursor_pointer_shadow = read_cursor(
        res::CURSOR_POINTER_SHADOW_PNG,
        &mut res_pak,
        &mut image_loader,
    );

    let cursor_pipeline = Arc::new(
        GraphicPipeline::create(
            &event_loop.device,
            GraphicPipelineInfo::new().blend(BlendMode::ALPHA),
            [
                Shader::new_vertex(
                    res_pak
                        .read_blob(res::SHADER_CURSOR_VERT_SPIRV)
                        .unwrap()
                        .as_slice(),
                ),
                Shader::new_fragment(
                    res_pak
                        .read_blob(res::SHADER_CURSOR_FRAG_SPIRV)
                        .unwrap()
                        .as_slice(),
                ),
            ],
        )
        .unwrap(),
    );
    let present_graphic_pipeline = Arc::new(
        GraphicPipeline::create(
            &event_loop.device,
            GraphicPipelineInfo::new(),
            [
                Shader::new_vertex(
                    res_pak
                        .read_blob(res::SHADER_PRESENT_VERT_SPIRV)
                        .unwrap()
                        .as_slice(),
                ),
                Shader::new_fragment(
                    res_pak
                        .read_blob(res::SHADER_PRESENT_FRAG_SPIRV)
                        .unwrap()
                        .as_slice(),
                ),
            ],
        )
        .unwrap(),
    );
    let mut transition_pipeline = TransitionPipeline::new(&event_loop.device);

    let mut ui: Option<Box<dyn Ui>> = Some(if args.benchmark {
        Box::new(Bench::boot(&event_loop.device))
    } else {
        Box::new(Boot::new(&event_loop.device))
    });

    let mut allow_cursor = true;
    let mut cursor = None;
    let mut keyboard = KeyBuf::default();
    let mut mouse = MouseBuf::default();

    event_loop
        .run(move |frame| {
            update_input(&mut keyboard, &mut mouse, frame.events);

            let mut dt = frame.dt;

            // Framerate limiter
            if !config.v_sync && !args.disable_framerate_limit {
                let framerate_limit = 1.0 / config.framerate_limit as f32;
                let started = Instant::now();
                while dt < framerate_limit {
                    dt = frame.dt + (Instant::now() - started).as_secs_f32();
                }
            }

            let framebuffer_height = if keyboard.is_held(&VirtualKeyCode::Tab) {
                frame.height
            } else {
                300
            };
            let framebuffer_width = frame.width * framebuffer_height / frame.height;
            let framebuffer_image = frame.render_graph.bind_node(
                pool.lease(ImageInfo::new_2d(
                    vk::Format::R8G8B8A8_UNORM,
                    framebuffer_width,
                    framebuffer_height,
                    vk::ImageUsageFlags::COLOR_ATTACHMENT
                        | vk::ImageUsageFlags::SAMPLED
                        | vk::ImageUsageFlags::STORAGE
                        | vk::ImageUsageFlags::TRANSFER_DST,
                ))
                .unwrap(),
            );
            let framebuffer_scale = (frame.width as f32 / framebuffer_width as f32)
                .max(frame.height as f32 / framebuffer_height as f32);

            ui = ui.take().unwrap().update(UpdateContext {
                audio: audio.as_mut(),
                config: &config,
                cursor: &mut cursor,
                dt,
                events: frame.events,
                framebuffer_aspect_ratio: framebuffer_width as f32 / framebuffer_height as f32,
                framebuffer_height,
                framebuffer_scale,
                framebuffer_width,
                keyboard: &keyboard,
                mouse: &mouse,
                window: frame.window,
            });

            if ui.is_none() {
                frame.render_graph.clear_color_image(frame.swapchain_image);
                *frame.will_exit = true;

                return;
            }

            ui.as_mut().unwrap().draw(DrawContext {
                dt,
                framebuffer_image,
                pool: &mut pool,
                render_graph: frame.render_graph,
                transition_pipeline: &mut transition_pipeline,
            });

            frame
                .render_graph
                .begin_pass("Present")
                .bind_pipeline(&present_graphic_pipeline)
                .read_descriptor(0, framebuffer_image)
                .store_color(0, frame.swapchain_image)
                .record_subpass(move |subpass, _| {
                    subpass.push_constants(cast_slice(
                        &Mat4::from_scale(vec3(
                            framebuffer_scale * framebuffer_width as f32 / frame.width as f32,
                            framebuffer_scale * framebuffer_height as f32 / frame.height as f32,
                            1.0,
                        ))
                        .to_cols_array(),
                    ));
                    subpass.draw(6, 1, 0, 0);
                });

            for event in frame.events {
                match event {
                    Event::WindowEvent {
                        event: WindowEvent::CursorLeft { .. },
                        ..
                    } => {
                        allow_cursor = false;
                    }
                    Event::WindowEvent {
                        event: WindowEvent::CursorEntered { .. },
                        ..
                    } => {
                        allow_cursor = true;
                    }
                    Event::WindowEvent {
                        event: WindowEvent::Focused(true),
                        ..
                    } => {
                        frame.window.set_cursor_visible(false);
                    }
                    _ => (),
                }
            }

            if allow_cursor {
                if let Some(cursor) = cursor {
                    let (mouse_x, mouse_y) = mouse.position();
                    let cursor_x = 2.0 * mouse_x / frame.width as f32 - 1.0;
                    let cursor_y = 2.0 * mouse_y / frame.height as f32 - 1.0;

                    let pixel_offset = match cursor {
                        CursorStyle::Pointer | CursorStyle::PointerShadow => 0.0,
                    };
                    let pixel_scale = 3.0;

                    let cursor_offset = pixel_scale * 2.0 * pixel_offset / frame.width as f32;

                    let cursor = match cursor {
                        CursorStyle::Pointer => &cursor_pointer,
                        CursorStyle::PointerShadow => &cursor_pointer_shadow,
                    };

                    let cursor_scale = pixel_scale * cursor.info.width as f32 / frame.width as f32;
                    let cursor = frame.render_graph.bind_node(cursor);
                    let render_aspect_ratio = frame.render_aspect_ratio();
                    frame
                        .render_graph
                        .begin_pass("Cursor")
                        .bind_pipeline(&cursor_pipeline)
                        .read_descriptor(0, cursor)
                        .load_color(0, frame.swapchain_image)
                        .store_color(0, frame.swapchain_image)
                        .record_subpass(move |subpass, _| {
                            subpass
                                .push_constants(&bytes_of(&vec4(
                                    cursor_x + cursor_scale - cursor_offset,
                                    cursor_y + cursor_scale * render_aspect_ratio
                                        - cursor_offset * render_aspect_ratio,
                                    cursor_scale,
                                    cursor_scale * render_aspect_ratio,
                                )))
                                .draw(6, 1, 0, 0);
                        });
                }
            }
        })
        .unwrap();

    trace!("OK");
}

fn read_cursor(key: &str, res_pak: &mut PakBuf, image_loader: &mut ImageLoader) -> Arc<Image> {
    let bitmap = res_pak.read_bitmap(key).unwrap();

    debug_assert_eq!(bitmap.format(), BitmapFormat::Rgba);

    image_loader
        .decode_linear(
            0,
            0,
            bitmap.pixels(),
            ImageFormat::R8G8B8A8,
            bitmap.width(),
            bitmap.height(),
        )
        .unwrap()
}

fn read_icon(key: &str, res_pak: &mut PakBuf) -> Icon {
    let bitmap = res_pak.read_bitmap(key).unwrap();

    debug_assert_eq!(bitmap.format(), BitmapFormat::Rgba);

    Icon::from_rgba(bitmap.pixels().to_vec(), bitmap.width(), bitmap.height()).unwrap()
}

/// Makes sure that any thread which panics causes the program to exit.
fn set_thread_panic_hook() {
    let orig_hook = take_hook();

    set_hook(Box::new(move |panic_info| {
        orig_hook(panic_info);

        exit(1);
    }));
}
