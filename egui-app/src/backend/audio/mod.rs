pub mod analyser;
mod decode;
mod device;
pub mod engine;
mod eq;
mod media_controls;
mod resample;
pub mod state;
mod tick;
mod timing;
mod types;

pub use analyser::start_fft_thread;
pub use device::start_default_output_monitor;
// Phase 4 で復元 (souvlaki は HWND 必須のため boot で起動しない)。
// pub use media_controls::start_media_controls;
pub use state::init;
pub use tick::start_tick_emitter;
