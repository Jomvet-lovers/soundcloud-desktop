//! Direct mode: the app talks to SoundCloud itself instead of the developer
//! backend. Reads go to api-v2, writes are stored locally (see `store`) and
//! are best-effort synced to SoundCloud through the hidden writer webview
//! (see `webview`).

pub mod routes;
pub mod sc;
pub mod store;
pub mod webview;

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use tauri::State;
use tokio::sync::Mutex;

use crate::auth::SessionStore;
use crate::rt::AppHandle;
use store::LocalStore;

pub struct DirectState {
    pub http: wreq::Client,
    pub app: AppHandle,
    pub client_id: Mutex<Option<String>>,
    pub me_cache: Mutex<Option<sc::MeCache>>,
    pub store: Mutex<LocalStore>,
}

impl DirectState {
    pub fn init(data_dir: PathBuf, http: wreq::Client, app: AppHandle) -> Arc<Self> {
        let store = LocalStore::load(&data_dir.join("direct_store.json"));
        Arc::new(Self {
            http,
            app,
            client_id: Mutex::new(None),
            me_cache: Mutex::new(None),
            store: Mutex::new(store),
        })
    }
}

/// Validate an `oauth_token` against SoundCloud and persist it as the session.
#[tauri::command]
pub async fn direct_login(
    token: String,
    app: AppHandle,
    session: State<'_, Arc<SessionStore>>,
    direct: State<'_, Arc<DirectState>>,
) -> Result<String, String> {
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err("empty token".into());
    }
    let me = sc::fetch_me(direct.inner(), &token).await?;
    let username = me
        .get("username")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    session.set_token(&app, token).await?;
    Ok(username)
}
