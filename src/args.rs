use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Run in benchmarking mode (instead of game mode)
    #[arg(long, default_value_t = false)]
    pub benchmark: bool,

    /// Enable Vulkan debug layers
    #[arg(long, default_value_t = false)]
    #[cfg(debug_assertions)]
    pub debug_vulkan: bool,

    /// Disable the framerate limit (has no effect when v-sync is enabled)
    #[arg(long, default_value_t = false)]
    pub disable_framerate_limit: bool,

    /// Disable ray tracing graphics
    #[arg(long, default_value_t = false)]
    pub disable_ray_tracing: bool,

    /// Disable audio
    #[arg(long, default_value_t = false)]
    pub mute: bool,

    /// Run in windowed mode
    #[arg(long, default_value_t = false)]
    pub window: bool,
}
