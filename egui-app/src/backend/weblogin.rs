//! In-app SoundCloud login window (wry version).
//! Port of Tauri `direct/login.rs`: a visible window at soundcloud.com
//! picks up the `oauth_token` cookie after sign-in, validates it via
//! `/me`, and persists it through `SessionStore`.
//!
//! The window always opens signed out (profile wipe before display).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{json, Value};

use crate::backend::auth::SessionStore;
use crate::backend::direct::DirectState;
use crate::backend::events::EventBus;

use super::webhost::{self, HostProxy, LOGIN_HOME};

/// Frontend event: `{ status: "ok" | "error" | "cancel", ... }`.
pub const EVENT: &str = "sc-login";
const POLL_MS: u64 = 1000;
const CLEAR_ATTEMPTS: usize = 10;
const MAX_COOKIE_ERRORS: u32 = 30;

static WATCHING: AtomicBool = AtomicBool::new(false);

/// Open (or focus) the login window and watch for its cookie.
/// `rt` must be the app runtime: this is called from the egui UI thread,
/// which has no ambient Tokio context.
pub fn open_login_window(
    rt: &tokio::runtime::Handle,
    direct: Arc<DirectState>,
    session: Arc<SessionStore>,
    bus: EventBus,
) {
    let Some(host) = webhost::host() else {
        bus.emit(
            EVENT,
            &json!({ "status": "error", "message": "webview unavailable" }),
        );
        return;
    };
    if WATCHING.swap(true, Ordering::SeqCst) {
        let h = host.clone();
        rt.spawn(async move {
            h.login_show().await;
            h.login_focus().await;
        });
        return;
    }
    rt.spawn(async move {
        watch(host, direct, session, bus).await;
        WATCHING.store(false, Ordering::SeqCst);
    });
}

fn oauth_token(cookies: &[(String, String)]) -> Option<String> {
    cookies
        .iter()
        .find(|(n, _)| n.contains("oauth_token"))
        .map(|(_, v)| v.clone())
}

async fn clear_previous_session(host: &HostProxy) -> Option<String> {
    tokio::time::sleep(Duration::from_millis(300)).await;

    let stale = host
        .login_cookies()
        .await
        .ok()
        .and_then(|cs| oauth_token(&cs));
    if stale.is_some() {
        eprintln!("[login] stale oauth_token found; wiping the web session");
    }

    for attempt in 0..CLEAR_ATTEMPTS {
        if let Err(e) = host.login_clear().await {
            eprintln!("[login] clear browsing data failed: {e}");
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
        match host.login_cookies().await {
            Ok(cookies) if oauth_token(&cookies).is_none() => {
                eprintln!("[login] previous web session cleared");
                break;
            }
            _ if attempt + 1 == CLEAR_ATTEMPTS => {
                eprintln!("[login] WARNING: oauth_token still present after wipe");
            }
            _ => {}
        }
    }

    let _ = host.login_navigate(LOGIN_HOME).await;
    stale
}

async fn watch(host: HostProxy, direct: Arc<DirectState>, session: Arc<SessionStore>, bus: EventBus) {
    let stale = clear_previous_session(&host).await;
    host.login_show().await;
    host.login_focus().await;

    let mut last_tried: Option<String> = stale;
    let mut cookie_errors: u32 = 0;

    loop {
        tokio::time::sleep(Duration::from_millis(POLL_MS)).await;

        match host.login_visible().await {
            Ok(true) => {}
            _ => {
                bus.emit(EVENT, &json!({ "status": "cancel" }));
                return;
            }
        }

        let cookies = match host.login_cookies().await {
            Ok(c) => {
                cookie_errors = 0;
                c
            }
            Err(_) => {
                cookie_errors += 1;
                if cookie_errors >= MAX_COOKIE_ERRORS {
                    bus.emit(
                        EVENT,
                        &json!({ "status": "error", "message": "cookie access unavailable" }),
                    );
                    return;
                }
                continue;
            }
        };

        let Some(token) = oauth_token(&cookies) else {
            continue;
        };
        if token.is_empty() || last_tried.as_deref() == Some(token.as_str()) {
            continue;
        }

        match super::direct::sc::fetch_me(direct.as_ref(), &token).await {
            Ok(me) => {
                let username = me
                    .get("username")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if let Err(e) = session.set_token(&bus, token.clone()).await {
                    bus.emit(EVENT, &json!({ "status": "error", "message": e }));
                    return;
                }
                bus.emit(
                    EVENT,
                    &json!({ "status": "ok", "token": token, "username": username }),
                );
                host.login_close().await;
                return;
            }
            Err(msg) => {
                if msg.contains("401") || msg.contains("403") {
                    eprintln!("[login] stale oauth_token; waiting for a fresh one");
                    last_tried = Some(token);
                }
            }
        }
    }
}
