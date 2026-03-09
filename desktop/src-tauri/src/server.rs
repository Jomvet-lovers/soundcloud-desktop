use std::path::PathBuf;

pub struct ServerState {
    pub audio_port: u16,
    pub proxy_port: u16,
}

#[tauri::command]
pub fn get_server_ports(state: tauri::State<'_, std::sync::Arc<ServerState>>) -> (u16, u16) {
    (state.audio_port, state.proxy_port)
}

pub fn cors() -> warp::cors::Builder {
    warp::cors()
        .allow_any_origin()
        .allow_methods(vec![
            "GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS",
        ])
        .allow_headers(vec![
            "range",
            "content-type",
            "accept",
            "authorization",
            "accept-encoding",
        ])
        .expose_headers(vec!["content-range", "content-length", "accept-ranges"])
}

/// Starts audio + proxy servers on separate ports.
/// Returns (audio_port, proxy_port).
pub async fn start_all(cache_dir: PathBuf) -> (u16, u16) {
    let audio_port = crate::audio_server::start(cache_dir).await;
    let proxy_port = crate::proxy_server::start().await;
    (audio_port, proxy_port)
}
