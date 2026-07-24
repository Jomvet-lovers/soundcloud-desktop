//! Рантайм-абстракция приложения.
//!
//! Фича `cef` переключает весь апп на рантайм CEF (Chromium) вместо wry
//! (webkitgtk/webview2). Ветка `feat/cef` убрала дефолтный параметр рантайма
//! у публичных типов Tauri, поэтому «голые» `AppHandle`/`WebviewWindow`/`App`
//! по коду берём отсюда — уже привязанными к выбранному рантайму.

#[cfg(feature = "cef")]
pub type Rt = tauri::Cef;
#[cfg(not(feature = "cef"))]
pub type Rt = tauri::Wry;

pub type AppHandle = tauri::AppHandle<Rt>;
pub type WebviewWindow = tauri::WebviewWindow<Rt>;
pub type App = tauri::App<Rt>;
