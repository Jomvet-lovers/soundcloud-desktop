//! In-app SoundCloud login window.
//!
//! Opens a visible webview at soundcloud.com; once the user signs in, the
//! `oauth_token` cookie is picked up automatically, validated against `/me`
//! and persisted through [`SessionStore`] — the same path as the manual
//! token paste in [`super::direct_login`], without the DevTools detour.
//!
//! The WebView2 profile is persistent, so the SoundCloud web session
//! survives app restarts and a later re-login is one click.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{Emitter, Manager};

use crate::auth::SessionStore;
use crate::rt::{AppHandle, WebviewWindow};

use super::DirectState;

pub const LABEL: &str = "sc-login";
/// Emitted to the frontend: `{ status: "ok" | "error" | "cancel", … }`.
const EVENT: &str = "sc-login";
const HOME: &str = "https://soundcloud.com/signin";
const POLL_MS: u64 = 1000;
/// Consecutive cookie-read failures before giving up (unsupported platform).
const MAX_COOKIE_ERRORS: u32 = 30;

/// Guards against spawning a second watcher when the button is clicked again
/// while the window is already open.
static WATCHING: AtomicBool = AtomicBool::new(false);

/// Open (or focus) the SoundCloud login window and watch for its cookie.
#[tauri::command]
pub async fn open_login_window(app: AppHandle) -> Result<(), String> {
    let _ = window(&app)?;
    if !WATCHING.swap(true, Ordering::SeqCst) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            watch(app).await;
            WATCHING.store(false, Ordering::SeqCst);
        });
    }
    Ok(())
}

fn window(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return Ok(w);
    }
    let url: tauri::Url = HOME.parse().map_err(|e| format!("{e}"))?;
    tauri::WebviewWindowBuilder::new(app, LABEL, tauri::WebviewUrl::External(url))
        .title("SoundCloud")
        .inner_size(1000.0, 800.0)
        .min_inner_size(480.0, 600.0)
        .zoom_hotkeys_enabled(true)
        .center()
        .focused(true)
        .build()
        .map_err(|e| e.to_string())
}

fn emit(app: &AppHandle, payload: Value) {
    app.emit(EVENT, payload).ok();
}

/// Poll the window cookies until a usable `oauth_token` shows up, the user
/// closes the window, or cookie access is unsupported.
async fn watch(app: AppHandle) {
    let mut last_tried: Option<String> = None;
    let mut cookie_errors: u32 = 0;

    loop {
        tokio::time::sleep(Duration::from_millis(POLL_MS)).await;

        let Some(wv) = app.get_webview_window(LABEL) else {
            // Closed by the user without completing the sign-in.
            emit(&app, json!({ "status": "cancel" }));
            return;
        };

        let Ok(url) = HOME.parse::<tauri::Url>() else {
            return;
        };
        let cookies = match wv.cookies_for_url(url) {
            Ok(c) => {
                cookie_errors = 0;
                c
            }
            Err(e) => {
                cookie_errors += 1;
                if cookie_errors == 1 {
                    eprintln!("[login] cookies_for_url failed: {e}");
                }
                if cookie_errors >= MAX_COOKIE_ERRORS {
                    emit(
                        &app,
                        json!({ "status": "error", "message": "cookie access unavailable" }),
                    );
                    return;
                }
                continue;
            }
        };

        let Some(token) = cookies
            .iter()
            .find(|c| c.name() == "oauth_token")
            .map(|c| c.value().to_string())
        else {
            continue;
        };
        if token.is_empty() || last_tried.as_deref() == Some(token.as_str()) {
            continue;
        }

        let direct = app.state::<Arc<DirectState>>().inner().clone();
        match super::sc::fetch_me(direct.as_ref(), &token).await {
            Ok(me) => {
                let username = me
                    .get("username")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let session = app.state::<Arc<SessionStore>>().inner().clone();
                if let Err(e) = session.set_token(&app, token.clone()).await {
                    emit(&app, json!({ "status": "error", "message": e }));
                    return;
                }
                emit(
                    &app,
                    json!({ "status": "ok", "token": token, "username": username }),
                );
                let _ = wv.close();
                return;
            }
            Err(msg) => {
                if msg.contains("401") || msg.contains("403") {
                    // Anonymous/stale cookie: remember it and wait for a
                    // fresh one to appear after the user signs in.
                    eprintln!("[login] stale oauth_token; waiting for a fresh one");
                    last_tried = Some(token);
                }
                // Other errors (network, 5xx) retry with the same token.
            }
        }
    }
}
