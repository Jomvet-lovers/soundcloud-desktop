//! C-ABI call-агента relay-сети: оболочка поднимает агент (автостарт по флагу),
//! бэкенд использует ноду в relay-сети. Тумблер/статус — опционально для UI.

use std::ffi::c_char;
use std::sync::OnceLock;

use call_agent::CallAgent;

use crate::cstr;

static CALL: OnceLock<CallAgent> = OnceLock::new();

/// Поднять call-агент (автостарт по флагу `call_enabled.json` в [data_dir]).
/// Идемпотентно; `false` — не удалось создать рантайм.
/// # Safety
/// `data_dir` — валидная NUL-терминированная C-строка либо null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sc_call_start(data_dir: *const c_char) -> bool {
    if CALL.get().is_some() {
        return true;
    }
    let dir = unsafe { cstr(data_dir) };
    match CallAgent::new(dir) {
        Ok(agent) => {
            agent.autostart();
            CALL.set(agent).is_ok()
        }
        Err(_) => false,
    }
}

/// Включить/выключить участие в relay-сети (персист флага + старт/стоп агента).
#[unsafe(no_mangle)]
pub extern "C" fn sc_call_set_enabled(enabled: bool) {
    if let Some(agent) = CALL.get() {
        agent.set_enabled(enabled);
    }
}

/// Статус: 0 disabled, 1 connecting, 2 provisioning, 3 active, 4 failed.
#[unsafe(no_mangle)]
pub extern "C" fn sc_call_status() -> i32 {
    CALL.get().map(|a| a.status_index()).unwrap_or(0)
}

/// Включён ли по флагу (для тумблера настроек).
#[unsafe(no_mangle)]
pub extern "C" fn sc_call_is_enabled() -> bool {
    CALL.get().map(|a| a.is_enabled()).unwrap_or(false)
}
