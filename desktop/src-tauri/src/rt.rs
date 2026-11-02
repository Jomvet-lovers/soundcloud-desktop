//! Application runtime abstraction.
//!
//! The `cef` feature switches the whole app to the CEF (Chromium) runtime instead of wry
//! (webkitgtk/webview2). The `feat/cef` branch removed the default runtime parameter
//! from Tauri's public types, so the "bare" `AppHandle`/`WebviewWindow`/`App`
//! are taken from here throughout the code — already bound to the selected runtime.

#[cfg(feature = "cef")]
pub type Rt = tauri::Cef;
#[cfg(not(feature = "cef"))]
pub type Rt = tauri::Wry;

pub type AppHandle = tauri::AppHandle<Rt>;
pub type WebviewWindow = tauri::WebviewWindow<Rt>;
pub type App = tauri::App<Rt>;
