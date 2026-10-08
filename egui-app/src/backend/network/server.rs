use std::path::PathBuf;

pub struct ServerState {
    pub static_port: u16,
    pub proxy_port: u16,
    pub api_port: u16,
}

// TODO(egui-migration): 元は `#[tauri::command]` + `tauri::State<'_, Arc<ServerState>>`。
// egui からは内部 API を直接呼ぶため `&ServerState` を受ける通常関数化。
pub fn get_server_ports(state: &ServerState) -> (u16, u16, u16) {
    (state.static_port, state.proxy_port, state.api_port)
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
            "x-session-id",
        ])
        .expose_headers(vec!["content-range", "content-length", "accept-ranges"])
}

/// `wallpapers_dir` には呼び出し側で
/// `crate::backend::paths::app_cache_dir().join("wallpapers")` を渡す
/// (元 `lib.rs` の Tauri `app.path().app_cache_dir()` 対応)。
pub async fn start_all(wallpapers_dir: PathBuf) -> (u16, u16) {
    let static_port = crate::backend::network::static_server::start(wallpapers_dir).await;
    let proxy_port = crate::backend::network::proxy_server::start().await;
    (static_port, proxy_port)
}
