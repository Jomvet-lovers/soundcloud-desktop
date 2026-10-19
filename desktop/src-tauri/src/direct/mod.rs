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
use tokio::sync::{Mutex, RwLock};

use crate::rt::AppHandle;
use store::LocalStore;

pub struct DirectState {
    pub http: wreq::Client,
    pub app: AppHandle,
    pub client_id: Mutex<Option<String>>,
    /// `/me` cache keyed by oauth token.
    pub me_cache: Mutex<HashMap<String, sc::MeCache>>,
    pub data_dir: PathBuf,
    /// Local stores keyed by owner (`None` = anonymous, never persisted).
    pub stores: RwLock<HashMap<Option<u64>, Arc<Mutex<LocalStore>>>>,
    /// SoundCloud user id of the active session (`None` = signed out).
    pub active_owner: RwLock<Option<u64>>,
    /// Short-lived cache of sorted search windows (key: query + sort).
    pub search_cache: Mutex<HashMap<String, (Instant, Vec<Value>)>>,
    /// Playlist URNs already checked against SoundCloud this session, so the
    /// local-copy self-heal below fetches each remote detail at most once.
    pub verified_playlists: Mutex<HashSet<String>>,
}

impl DirectState {
    pub fn init(data_dir: PathBuf, http: wreq::Client, app: AppHandle) -> Arc<Self> {
        Arc::new(Self {
            http,
            app,
            client_id: Mutex::new(None),
            me_cache: Mutex::new(HashMap::new()),
            data_dir,
            stores: RwLock::new(HashMap::new()),
            active_owner: RwLock::new(None),
            search_cache: Mutex::new(HashMap::new()),
            verified_playlists: Mutex::new(HashSet::new()),
        })
    }

    pub fn store_path(&self, owner: u64) -> PathBuf {
        self.data_dir.join(format!("direct_store.{owner}.json"))
    }

    /// Store for `owner`, loaded from `direct_store.<owner>.json` on first use.
    /// The anonymous store has no path, so `LocalStore::save` never persists it.
    pub async fn store_for(&self, owner: Option<u64>) -> Arc<Mutex<LocalStore>> {
        if let Some(store) = self.stores.read().await.get(&owner) {
            return store.clone();
        }
        let mut stores = self.stores.write().await;
        if let Some(store) = stores.get(&owner) {
            return store.clone();
        }
        let store = match owner {
            Some(id) => LocalStore::load(&self.store_path(id)),
            None => LocalStore::default(),
        };
        let store = Arc::new(Mutex::new(store));
        stores.insert(owner, store.clone());
        store
    }

    /// Resolve the request's account: the token's SoundCloud user id when a
    /// token is present (cached after the first `/me`), otherwise the active
    /// account bound by the auth layer.
    pub async fn resolve_owner(&self, token: Option<&str>) -> Option<u64> {
        match token.filter(|t| !t.is_empty()) {
            Some(t) => self.my_user_id(t).await.ok(),
            None => *self.active_owner.read().await,
        }
    }

    pub async fn rebind_owner(&self, id: Option<u64>) {
        *self.active_owner.write().await = id;
    }
}

