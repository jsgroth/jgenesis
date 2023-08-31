pub mod config;
mod genesisinput;
mod input;
mod mainloop;
mod renderer;

pub use mainloop::{run_genesis, run_smsgg};
pub use renderer::config::{FilterMode, PrescaleFactor, RendererConfig, VSyncMode};
