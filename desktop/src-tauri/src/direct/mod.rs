//! Direct mode: the app talks to SoundCloud itself instead of the developer
//! backend. Reads go to api-v2, writes are stored locally (see `store`) and
//! are best-effort synced to SoundCloud through the hidden writer webview
//! (see `webview`).

pub mod login;
pub mod routes;
pub mod sc;
pub mod store;
pub mod webview;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;
use tokio::sync::Mutex;

use crate::rt::AppHandle;
use store::LocalStore;

pub struct DirectState {
    pub http: wreq::Client,
    pub app: AppHandle,
    pub client_id: Mutex<Option<String>>,
    pub me_cache: Mutex<Option<sc::MeCache>>,
    pub store: Mutex<LocalStore>,
    /// Short-lived cache of sorted search windows (key: query + sort).
    pub search_cache: Mutex<HashMap<String, (Instant, Vec<Value>)>>,
    /// Playlist URNs already checked against SoundCloud this session, so the
    /// local-copy self-heal below fetches each remote detail at most once.
    pub verified_playlists: Mutex<HashSet<String>>,
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
            search_cache: Mutex::new(HashMap::new()),
            verified_playlists: Mutex::new(HashSet::new()),
        })
    }
}

