//! Phase 1 移植層: `desktop/src-tauri` の Tauri 非依存ロジックを egui 側へ。
//!
//! - `events::EventBus` が Tauri の `AppHandle` + `Emitter` を代替する。
//! - `writer` は Phase 4 (wry writer) までの縮退スタブ。
//! - `drop 対象`: `rt.rs` (Tauri 型別名)、`commands.rs` 群 (`#[tauri::command]`
//!   薄皮。egui からは内部 API を直接呼ぶ。対応表は各移植報告に記録)、
//!   `direct/{login,webview}.rs` (Phase 4)、`app/tray.rs` (Phase 5)、
//!   `lib.rs` / `main.rs` の Builder・setup (Phase 2 で `boot()` として再建)。

pub mod app;
pub mod api;
pub mod audio;
pub mod auth;
pub mod boot;
pub mod direct;
pub mod discord;
pub mod events;
pub mod models;
pub mod network;
pub mod paths;
pub mod shared;
pub mod track_cache;
pub mod writer;
