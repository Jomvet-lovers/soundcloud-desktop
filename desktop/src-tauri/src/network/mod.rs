pub mod audio_route;
pub mod edge;
pub mod image_cache;
pub mod proxy;
pub mod proxy_server;
pub mod server;
pub mod static_server;

/// Browser User-Agent for hosts that reject non-browser clients.
pub const BROWSER_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";
