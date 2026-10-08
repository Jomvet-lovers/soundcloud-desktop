//! Direct mode: the app talks to SoundCloud itself instead of the developer
//! backend. Reads go to api-v2, writes are stored locally (see `store`) and
//! are best-effort synced to SoundCloud through the hidden writer webview
//! (see `crate::backend::writer`).

pub mod routes;
pub mod sc;
pub mod store;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;
use tokio::sync::Mutex;

use store::LocalStore;

pub struct DirectState {
    pub http: wreq::Client,
    pub app: crate::backend::events::EventBus,
    pub client_id: Mutex<Option<String>>,
    pub me_cache: Mutex<Option<sc::MeCache>>,
    pub store: Mutex<LocalStore>,
    /// Short-lived cache of sorted search windows (key: query + sort).
    pub search_cache: Mutex<HashMap<String, (Instant, Vec<Value>)>>,
}

impl DirectState {
    pub fn init(
        data_dir: PathBuf,
        http: wreq::Client,
        app: crate::backend::events::EventBus,
    ) -> Arc<Self> {
        let store = LocalStore::load(&data_dir.join("direct_store.json"));
        Arc::new(Self {
            http,
            app,
            client_id: Mutex::new(None),
            me_cache: Mutex::new(None),
            store: Mutex::new(store),
            search_cache: Mutex::new(HashMap::new()),
        })
    }
}
