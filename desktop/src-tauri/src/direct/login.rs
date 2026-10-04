//! In-app SoundCloud login window.
//!
//! Opens a visible webview at soundcloud.com; once the user signs in, the
//! `oauth_token` cookie is picked up automatically, validated against `/me`
//! and persisted through [`SessionStore`] — the same path as the manual
//! token paste in [`super::direct_login`], without the DevTools detour.
//!
//! The window always opens signed out: the persistent WebView profile would
//! otherwise keep the previous SoundCloud session and prevent switching
//! accounts. The whole web browsing profile (cookies, storage, service
//! workers) is wiped before the sign-in page is shown.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::webview::Cookie;
use tauri::{Emitter, Manager};

use crate::auth::SessionStore;
use crate::rt::{AppHandle, WebviewWindow};

use super::DirectState;

pub const LABEL: &str = "sc-login";
/// Emitted to the frontend: `{ status: "ok" | "error" | "cancel", … }`.
const EVENT: &str = "sc-login";
const HOME: &str = "https://soundcloud.com/signin";
const POLL_MS: u64 = 1000;
/// Sign-out polling budget (300 ms apart) while the webview warms up.
const CLEAR_ATTEMPTS: usize = 10;
/// Consecutive cookie-read failures before giving up (unsupported platform).
const MAX_COOKIE_ERRORS: u32 = 30;

/// Guards against spawning a second watcher when the button is clicked again
/// while the window is already open.
static WATCHING: AtomicBool = AtomicBool::new(false);

/// Open (or focus) the SoundCloud login window and watch for its cookie.
#[tauri::command]
pub async fn open_login_window(app: AppHandle) -> Result<(), String> {
    let w = window(&app)?;
    if WATCHING.swap(true, Ordering::SeqCst) {
        // A watcher is already running (window open or still clearing) —
        // just bring the window to the front.
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    // Keep the window hidden until the previous web session is cleared so the
    // old account never flashes on screen.
    let _ = w.hide();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        watch(app).await;
        WATCHING.store(false, Ordering::SeqCst);
    });
    Ok(())
}

fn window(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.unminimize();
        return Ok(w);
    }
    let url: tauri::Url = HOME.parse().map_err(|e| format!("{e}"))?;
    tauri::WebviewWindowBuilder::new(app, LABEL, tauri::WebviewUrl::External(url))
        .title("SoundCloud")
        .inner_size(1000.0, 800.0)
        .min_inner_size(480.0, 600.0)
        .zoom_hotkeys_enabled(true)
        .center()
        .visible(false)
        .build()
        .map_err(|e| e.to_string())
}

fn emit(app: &AppHandle, payload: Value) {
    app.emit(EVENT, payload).ok();
}

fn home_url() -> Option<tauri::Url> {
    HOME.parse().ok()
}

/// SoundCloud auth cookies currently held by the login webview.
fn auth_cookies(wv: &WebviewWindow) -> Option<Vec<Cookie<'static>>> {
    let url = home_url()?;
    wv.cookies_for_url(url).ok().map(|cookies| {
        cookies
            .into_iter()
            .filter(|c| c.name().contains("oauth_token"))
            .collect()
    })
}

/// Sign the persistent web session out by wiping the login window's browsing
/// data (cookies, localStorage, IndexedDB, service workers, cache). Removing
/// just the `oauth_token` cookie is not enough: the SoundCloud web app keeps
/// refresh state outside cookies and silently re-authenticates, which made the
/// window open already signed in. Returns the stale token so the watcher can
/// refuse to accept it again.
async fn clear_previous_session(app: &AppHandle) -> Option<String> {
    // Let the freshly created webview spin up before touching its profile.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let Some(wv) = app.get_webview_window(LABEL) else {
        return None;
    };

    let stale = auth_cookies(&wv).and_then(|cookies| cookies.first().map(|c| c.value().to_string()));
    if stale.is_some() {
        eprintln!("[login] stale oauth_token found; wiping the web session");
    }

    for attempt in 0..CLEAR_ATTEMPTS {
        if let Err(e) = wv.clear_all_browsing_data() {
            eprintln!("[login] clear_all_browsing_data failed: {e}");
        }
        // The profile wipe is asynchronous; give it a beat, then verify.
        tokio::time::sleep(Duration::from_millis(400)).await;
        match auth_cookies(&wv) {
            Some(cookies) if cookies.is_empty() => {
                eprintln!("[login] previous web session cleared");
                break;
            }
            _ if attempt + 1 == CLEAR_ATTEMPTS => {
                eprintln!("[login] WARNING: oauth_token still present after wipe");
            }
            _ => {}
        }
    }

    // Reload the sign-in page with the clean profile.
    if let Some(url) = home_url() {
        let _ = wv.navigate(url);
    }
    stale
}

/// Poll the window cookies until a usable `oauth_token` shows up, the user
/// closes the window, or cookie access is unsupported.
async fn watch(app: AppHandle) {
    let stale = clear_previous_session(&app).await;
    if let Some(wv) = app.get_webview_window(LABEL) {
        let _ = wv.show();
        let _ = wv.set_focus();
    }

    // The stale token must never complete the flow: only a freshly issued one
    // (the user actually signed in) is accepted.
    let mut last_tried: Option<String> = stale;
    let mut cookie_errors: u32 = 0;

    loop {
        tokio::time::sleep(Duration::from_millis(POLL_MS)).await;

        let Some(wv) = app.get_webview_window(LABEL) else {
            // Closed by the user without completing the sign-in.
            emit(&app, json!({ "status": "cancel" }));
            return;
        };

        let Some(cookies) = auth_cookies(&wv) else {
            cookie_errors += 1;
            if cookie_errors >= MAX_COOKIE_ERRORS {
                emit(
                    &app,
                    json!({ "status": "error", "message": "cookie access unavailable" }),
                );
                return;
            }
            continue;
        };
        cookie_errors = 0;

        let Some(token) = cookies.first().map(|c| c.value().to_string()) else {
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
