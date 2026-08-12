//! Единый десктоп-FFI: C-ABI поверх десктоп-онли крейтов (media-controls,
//! discord-rpc, …), который Flutter-оболочка грузит через `dart:ffi`. Аналог
//! `sc-bridge` для Core, но для платформенных фич — Core от Desktop не зависит,
//! поэтому мост отдельный. Один экземпляр на процесс.

use std::ffi::{CStr, c_char};

mod call;
mod discord;
mod media;
mod tray;

/// C-строка → `String` (null → пусто). Общий помощник C-ABI модулей.
///
/// # Safety
/// `ptr` — валидная NUL-терминированная C-строка либо null.
pub(crate) unsafe fn cstr(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}
