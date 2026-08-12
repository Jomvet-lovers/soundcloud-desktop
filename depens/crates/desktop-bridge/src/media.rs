//! C-ABI медиа-контролов: оболочка драйвит MPRIS по событиям движка (outbound) и
//! получает медиа-клавиши обратно (inbound) через зарегистрированный колбэк.

use std::ffi::c_char;
use std::sync::{Mutex, OnceLock};

use media_controls::{DesktopMediaControls, MediaKey};

use crate::cstr;

static CONTROLS: OnceLock<DesktopMediaControls> = OnceLock::new();
static EVENT_CB: Mutex<Option<extern "C" fn(i32)>> = Mutex::new(None);

/// Код медиа-клавиши для Dart (зеркало [`MediaKey`]).
fn key_code(key: MediaKey) -> i32 {
    match key {
        MediaKey::Play => 0,
        MediaKey::Pause => 1,
        MediaKey::Toggle => 2,
        MediaKey::Next => 3,
        MediaKey::Previous => 4,
        MediaKey::Stop => 5,
    }
}

/// Зовётся на потоке медиа-контролов — пробрасываем в Dart, если хэндлер задан.
fn dispatch(key: MediaKey) {
    let cb = EVENT_CB.lock().ok().and_then(|g| *g);
    if let Some(cb) = cb {
        cb(key_code(key));
    }
}

/// Инициализировать системные медиа-контролы. Идемпотентно; `false` — не вышло.
#[unsafe(no_mangle)]
pub extern "C" fn sc_media_init() -> bool {
    if CONTROLS.get().is_some() {
        return true;
    }
    match DesktopMediaControls::new(dispatch) {
        Ok(controls) => CONTROLS.set(controls).is_ok(),
        Err(_) => false,
    }
}

/// Зарегистрировать обработчик медиа-клавиш (inbound). `null` — снять.
#[unsafe(no_mangle)]
pub extern "C" fn sc_media_set_event_handler(cb: Option<extern "C" fn(i32)>) {
    if let Ok(mut guard) = EVENT_CB.lock() {
        *guard = cb;
    }
}

/// Новый трек: метаданные + обложка + длительность (мс).
/// # Safety
/// `title`/`artist`/`cover_url` — валидные NUL-терминированные C-строки либо null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sc_media_now_playing(
    title: *const c_char,
    artist: *const c_char,
    cover_url: *const c_char,
    duration_ms: i64,
) {
    let Some(controls) = CONTROLS.get() else {
        return;
    };
    let title = unsafe { cstr(title) };
    let artist = unsafe { cstr(artist) };
    let cover = unsafe { cstr(cover_url) };
    controls.set_metadata(&title, &artist, &cover, duration_ms);
}

#[unsafe(no_mangle)]
pub extern "C" fn sc_media_set_playing(playing: bool) {
    if let Some(controls) = CONTROLS.get() {
        controls.set_playing(playing);
    }
}

/// Текущая позиция (мс) — для скраббера MPRIS.
#[unsafe(no_mangle)]
pub extern "C" fn sc_media_set_position(position_ms: i64) {
    if let Some(controls) = CONTROLS.get() {
        controls.set_position(position_ms);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn sc_media_clear() {
    if let Some(controls) = CONTROLS.get() {
        controls.clear();
    }
}
