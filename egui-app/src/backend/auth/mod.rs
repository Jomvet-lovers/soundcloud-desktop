//! Single source of truth for the auth session token.
//!
//! Rust owns the token: mutations (login / QR / logout) flow through these
//! functions, the value is persisted atomically and revoked server-side on
//! logout, and every change is broadcast as `auth:changed` via `EventBus`.
//! Also persists the premium flag for offline-bootstrap routing to the star host.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::backend::app::diagnostics::log_native;
use crate::backend::events::EventBus;

const SESSION_FILE: &str = "auth_session.json";
const LEGACY_FILE: &str = "sc-auth.json";
const EVENT: &str = "auth:changed";

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct AuthState {
    token: Option<String>,
    #[serde(default)]
    premium: bool,
}

pub struct SessionStore {
    path: PathBuf,
    state: RwLock<AuthState>,
    http: wreq::Client,
    rt: tokio::runtime::Handle,
}

impl SessionStore {
    pub fn init(
        app_data_dir: PathBuf,
        http: wreq::Client,
        rt: tokio::runtime::Handle,
    ) -> Arc<Self> {
        let path = app_data_dir.join(SESSION_FILE);
        let state = load_state(&path)
            .or_else(|| migrate_legacy(&app_data_dir.join(LEGACY_FILE), &path))
            .unwrap_or_default();
        Arc::new(Self {
            path,
            state: RwLock::new(state),
            http,
            rt,
        })
    }

    /// 同期読取: UI が `x-session-id` 用に使う。ログイン/ログアウト時は
    /// `EventBus` 経由で通知されるため、UI 側で差分更新する。
    pub fn token(&self) -> Option<String> {
        self.state.try_read().ok().and_then(|s| s.token.clone())
    }

    /// Programmatic session set (direct-mode token login). Persists and
    /// broadcasts exactly like the `auth_set_session` function.
    pub async fn set_token(&self, bus: &EventBus, token: String) -> Result<(), String> {
        let new = AuthState {
            token: Some(token),
            premium: false,
        };
        let mut guard = self.state.write().await;
        *guard = new.clone();
        if let Err(e) = write_state(&self.path, &guard) {
            log_native("ERROR", format!("[auth] persist failed: {e}"));
        }
        bus.emit(EVENT, &new);
        Ok(())
    }
}

fn is_usable(token: &str) -> bool {
    !token.is_empty() && token != "undefined" && token != "null"
}

/// Token must be usable for the rest of the state to count: premium without a
/// token doesn't live.
fn load_state(path: &Path) -> Option<AuthState> {
    let bytes = std::fs::read(path).ok()?;
    let state = serde_json::from_slice::<AuthState>(&bytes).ok()?;
    match &state.token {
        Some(t) if is_usable(t) => Some(state),
        _ => None,
    }
}

/// Adopt a token from the old zustand-persist file so upgrading doesn't sign
/// everyone out. Old shape: `{"state":{"sessionId":"..."},...}`.
fn migrate_legacy(legacy: &Path, dest: &Path) -> Option<AuthState> {
    let bytes = std::fs::read(legacy).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let token = v.get("state")?.get("sessionId")?.as_str()?.to_string();
    if !is_usable(&token) {
        return None;
    }
    let state = AuthState {
        token: Some(token),
        premium: false,
    };
    let _ = write_state(dest, &state);
    Some(state)
}

/// Atomic write: tmp -> fsync -> rename; `token == None` removes the file.
/// Either the old state or the fully-written new one survives a crash — never
/// a partial.
fn write_state(path: &Path, state: &AuthState) -> std::io::Result<()> {
    if state.token.is_none() {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec(state).map_err(std::io::Error::other)?;
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

pub async fn auth_status(state: &Arc<SessionStore>) -> Result<AuthState, String> {
    Ok(state.state.read().await.clone())
}

pub async fn auth_set_session(
    token: String,
    bus: &EventBus,
    state: &Arc<SessionStore>,
) -> Result<(), String> {
    // premium reset: a new identity doesn't inherit the flag, the
    // /me/subscription recheck restores it within seconds.
    let new = AuthState {
        token: Some(token),
        premium: false,
    };
    // Persist + emit under the write lock so file/event order matches memory.
    let mut guard = state.state.write().await;
    *guard = new.clone();
    if let Err(e) = write_state(&state.path, &guard) {
        log_native("ERROR", format!("[auth] persist failed: {e}"));
    }
    bus.emit(EVENT, &new);
    Ok(())
}

pub async fn auth_set_premium(
    premium: bool,
    bus: &EventBus,
    state: &Arc<SessionStore>,
) -> Result<(), String> {
    let mut guard = state.state.write().await;
    if guard.token.is_none() || guard.premium == premium {
        return Ok(());
    }
    guard.premium = premium;
    if let Err(e) = write_state(&state.path, &guard) {
        log_native("ERROR", format!("[auth] premium persist failed: {e}"));
    }
    bus.emit(EVENT, &guard.clone());
    Ok(())
}

pub async fn auth_logout(
    api_base: String,
    bus: &EventBus,
    state: &Arc<SessionStore>,
) -> Result<(), String> {
    // Drop the session locally first — logout must always succeed locally,
    // independent of the network revoke below. Persist + emit under the lock.
    let old = {
        let mut guard = state.state.write().await;
        let old = std::mem::take(&mut *guard);
        if let Err(e) = write_state(&state.path, &AuthState::default()) {
            log_native("ERROR", format!("[auth] clear failed: {e}"));
        }
        bus.emit(EVENT, &AuthState::default());
        old
    };

    if let Some(token) = old.token {
        let http = state.http.clone();
        state.rt.spawn(async move {
            let url = format!("{}/auth/logout", api_base.trim_end_matches('/'));
            match http
                .post(url)
                .header("x-session-id", token)
                .timeout(Duration::from_secs(10))
                .send()
                .await
            {
                Ok(r) => log_native("INFO", format!("[auth] server logout {}", r.status())),
                Err(e) => log_native("WARN", format!("[auth] server logout failed: {e}")),
            }
        });
    }
    Ok(())
}
