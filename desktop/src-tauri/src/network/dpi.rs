use std::sync::OnceLock;

use dpi_desync::{Desync, Strategy};

static DESYNC: OnceLock<Desync> = OnceLock::new();

const PROBE_URL: &str = "https://soundcloud.com";

pub fn install(d: Desync) {
    let url = d.proxy_url();
    let enabled = d.is_enabled();
    if DESYNC.set(d).is_ok() {
        tracing::info!(proxy = %url, enabled, "dpi-desync ready");
    }
}

pub async fn probe_in_background() {
    if let Some(d) = DESYNC.get() {
        let s = d.probe(PROBE_URL).await;
        tracing::info!(?s, "dpi-desync probe done");
    }
}

pub fn proxy_url() -> Option<String> {
    DESYNC.get().map(|d| d.proxy_url())
}

/// Привязывает SOCKS-десинк к билдеру reqwest. Если sneak не поднялся —
/// возвращает билдер как есть. Сам тумблер enable/disable рулит уже SOCKS
/// внутри, поэтому клиент не нужно пересоздавать при переключении.
pub fn apply(b: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    match proxy_url().and_then(|u| reqwest::Proxy::all(u).ok()) {
        Some(p) => b.proxy(p),
        None => b,
    }
}

#[tauri::command]
pub fn dpi_set_enabled(enabled: bool) {
    if let Some(d) = DESYNC.get() {
        d.set_enabled(enabled);
    }
}

#[tauri::command]
pub fn dpi_is_enabled() -> bool {
    DESYNC.get().map(|d| d.is_enabled()).unwrap_or(false)
}

#[tauri::command]
pub fn dpi_strategy() -> String {
    let s = DESYNC.get().map(|d| d.strategy()).unwrap_or(Strategy::None);
    format!("{:?}", s)
}
