pub mod genius;
pub mod handlers;
pub mod lrclib;
pub mod musixmatch;
pub mod netease;
pub mod service;
pub mod util;
pub mod worker_client;

pub use handlers::router;
pub use service::LyricsService;
pub use worker_client::{EncodeOutcome, WorkerClient};
