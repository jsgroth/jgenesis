pub mod config;
mod genesisinput;
mod mainloop;
mod renderer;
mod smsgginput;

pub use mainloop::{run_genesis, run_smsgg};
