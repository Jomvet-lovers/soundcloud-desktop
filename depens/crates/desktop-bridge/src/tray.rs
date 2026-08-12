//! C-ABI системного трея поверх крейта `tray` (ksni/SNI). Оболочка регистрирует
//! обработчик действий (inbound) и поднимает трей с иконкой; клики пунктов меню /
//! левый клик иконки прилетают кодом в Dart-колбэк.

use std::ffi::c_char;
use std::sync::{Arc, Mutex, OnceLock};

use tray::TrayAction;

use crate::cstr;

static ACTION_CB: Mutex<Option<extern "C" fn(i32)>> = Mutex::new(None);
static STARTED: OnceLock<bool> = OnceLock::new();

/// Код действия для Dart (зеркало [`TrayAction`]).
fn action_code(action: TrayAction) -> i32 {
    match action {
        TrayAction::Show => 0,
        TrayAction::Mini => 1,
        TrayAction::PlayPause => 2,
        TrayAction::Prev => 3,
        TrayAction::Next => 4,
        TrayAction::Quit => 5,
        TrayAction::Activate => 6,
    }
}

/// Зовётся с потока трея (D-Bus) — пробрасываем в Dart, если хэндлер задан.
fn dispatch(action: TrayAction) {
    let cb = ACTION_CB.lock().ok().and_then(|g| *g);
    if let Some(cb) = cb {
        cb(action_code(action));
    }
}

/// Зарегистрировать обработчик действий трея (inbound). `null` — снять.
#[unsafe(no_mangle)]
pub extern "C" fn sc_tray_set_action_handler(cb: Option<extern "C" fn(i32)>) {
    if let Ok(mut guard) = ACTION_CB.lock() {
        *guard = cb;
    }
}

/// Поднять системный трей с иконкой из PNG-файла. Идемпотентно; `false` — не вышло
/// (нет SNI-host / D-Bus / иконки). Хэндлер лучше задать до вызова.
///
/// # Safety
/// `icon_path` — валидная NUL-терминированная C-строка либо null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sc_tray_init(icon_path: *const c_char) -> bool {
    if let Some(started) = STARTED.get() {
        return *started;
    }
    let path = unsafe { cstr(icon_path) };
    let ok = tray::spawn(&path, Arc::new(dispatch));
    let _ = STARTED.set(ok);
    ok
}
