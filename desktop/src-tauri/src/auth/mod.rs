//! Single source of truth for the auth session token.
//!
//! Rust owns the token: mutations (login / QR / logout) flow through these
//! commands, the value is persisted atomically and revoked server-side on
//! logout, and every change is broadcast as `auth:changed` to all webviews.
//! The frontend keeps a read-only mirror for the `x-session-id` header.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::RwLock;

use crate::app::diagnostics::log_native;

const SESSION_FILE: &str = "auth_session.json";
const LEGACY_FILE: &str = "sc-auth.json";
const EVENT: &str = "auth:changed";

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct AuthState {
    token: Option<String>,
}

pub struct SessionStore {
    path: PathBuf,
    token: RwLock<Option<String>>,
    http: reqwest::Client,
    rt: tokio::runtime::Handle,
}

impl SessionStore {
    pub fn init(
        app_data_dir: PathBuf,
        http: reqwest::Client,
        rt: tokio::runtime::Handle,
    ) -> Arc<Self> {
        let path = app_data_dir.join(SESSION_FILE);
        let token =
            load_token(&path).or_else(|| migrate_legacy(&app_data_dir.join(LEGACY_FILE), &path));
        Arc::new(Self {
            path,
            token: RwLock::new(token),
            http,
            rt,
        })
    }
}

fn is_usable(token: &str) -> bool {
    !token.is_empty() && token != "undefined" && token != "null"
}

fn load_token(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice::<AuthState>(&bytes)
        .ok()?
        .token
        .filter(|t| is_usable(t))
}

/// Adopt a token from the old zustand-persist file so upgrading doesn't sign
/// everyone out. Old shape: `{"state":{"sessionId":"..."},...}`.
fn migrate_legacy(legacy: &Path, dest: &Path) -> Option<String> {
    let bytes = std::fs::read(legacy).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let token = v.get("state")?.get("sessionId")?.as_str()?.to_string();
    if !is_usable(&token) {
        return None;
    }
    let _ = write_token(dest, Some(&token));
    Some(token)
}

/// Atomic write: tmp -> fsync -> rename; `None` removes the file. Either the
/// old token or the fully-written new one survives a crash — never a partial.
fn write_token(path: &Path, token: Option<&str>) -> std::io::Result<()> {
    let Some(token) = token else {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec(&AuthState {
        token: Some(token.to_string()),
    })
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
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

#[tauri::command]
pub async fn auth_status(state: State<'_, Arc<SessionStore>>) -> Result<AuthState, String> {
    Ok(AuthState {
        token: state.token.read().await.clone(),
    })
}

#[tauri::command]
pub async fn auth_set_session(
    token: String,
    app: AppHandle,
    state: State<'_, Arc<SessionStore>>,
) -> Result<(), String> {
    *state.token.write().await = Some(token.clone());
    if let Err(e) = write_token(&state.path, Some(&token)) {
        log_native(&app, "ERROR", format!("[auth] persist failed: {e}"));
    }
    app.emit(EVENT, AuthState { token: Some(token) }).ok();
    Ok(())
}

#[tauri::command]
pub async fn auth_logout(
    api_base: String,
    app: AppHandle,
    state: State<'_, Arc<SessionStore>>,
) -> Result<(), String> {
    // Drop the session locally first — logout must always succeed locally,
    // independent of the network revoke below.
    let old = state.token.write().await.take();
    if let Err(e) = write_token(&state.path, None) {
        log_native(&app, "ERROR", format!("[auth] clear failed: {e}"));
    }
    app.emit(EVENT, AuthState { token: None }).ok();

    if let Some(token) = old {
        let http = state.http.clone();
        let app = app.clone();
        state.rt.spawn(async move {
            let url = format!("{}/auth/logout", api_base.trim_end_matches('/'));
            match http
                .post(url)
                .header("x-session-id", token)
                .timeout(Duration::from_secs(10))
                .send()
                .await
            {
                Ok(r) => log_native(&app, "INFO", format!("[auth] server logout {}", r.status())),
                Err(e) => log_native(&app, "WARN", format!("[auth] server logout failed: {e}")),
            }
        });
    }
    Ok(())
}
