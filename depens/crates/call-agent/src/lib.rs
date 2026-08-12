//! Call-агент relay-сети (десктоп): поднимает `call-client` как ноду relay —
//! бэкенд ходит через неё. Порт легаси `src-tauri/network/call.rs`. **Чистая
//! логика, без C-ABI** — наружу её отдаёт `desktop-bridge`.
//!
//! `call-client` здесь — локальный path-dep (без реальной реализации он `disabled`,
//! агент сразу уходит в `Disabled`). Реальный клиент инжектится в проде/CI тем же
//! путём (как у Tauri). «Подключить» = поднять агент + автостарт по флагу.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use call_client::{
    AgentConfig, Identity, IdentityStore, ProvisionInput, provision, run_agent,
};
use tokio::runtime::Runtime;
use tokio::task::AbortHandle;

const FLAG_FILE: &str = "call_enabled.json";
const DEFAULT_ENDPOINT: &str = "https://call.scnative.space:444";
const DEFAULT_POW_BITS: u32 = 22;

#[derive(Clone, Debug)]
pub enum CallStatus {
    Disabled,
    Connecting,
    Provisioning,
    Active,
    Failed(String),
}

impl CallStatus {
    /// Код-индекс для FFI: 0 disabled, 1 connecting, 2 provisioning, 3 active, 4 failed.
    pub fn index(&self) -> i32 {
        match self {
            CallStatus::Disabled => 0,
            CallStatus::Connecting => 1,
            CallStatus::Provisioning => 2,
            CallStatus::Active => 3,
            CallStatus::Failed(_) => 4,
        }
    }
}

struct Inner {
    flag_path: PathBuf,
    status: Mutex<CallStatus>,
    cancel: Mutex<Option<AbortHandle>>,
}

/// Агент relay-сети с собственным tokio-рантаймом (агент долгоживущий, async).
pub struct CallAgent {
    rt: Runtime,
    inner: Arc<Inner>,
}

impl CallAgent {
    pub fn new(data_dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;
        Ok(Self {
            rt,
            inner: Arc::new(Inner {
                flag_path: data_dir.into().join(FLAG_FILE),
                status: Mutex::new(CallStatus::Disabled),
                cancel: Mutex::new(None),
            }),
        })
    }

    /// Автостарт, если флаг включён (по умолчанию ВКЛ — как легаси).
    pub fn autostart(&self) {
        if self.is_enabled() {
            self.spawn();
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.save_flag(enabled);
        if enabled {
            self.spawn();
        } else {
            self.abort();
            set_status(&self.inner, CallStatus::Disabled);
        }
    }

    pub fn is_enabled(&self) -> bool {
        match std::fs::read(&self.inner.flag_path) {
            Ok(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|v| v.get("enabled").and_then(|e| e.as_bool()))
                .unwrap_or(true),
            Err(_) => true,
        }
    }

    /// Текущий статус (индекс, см. [`CallStatus::index`]).
    pub fn status_index(&self) -> i32 {
        lock(&self.inner.status).index()
    }

    fn spawn(&self) {
        self.abort();
        let inner = self.inner.clone();
        let handle = self.rt.spawn(async move {
            if let Err(error) = run_loop(inner.clone()).await {
                set_status(&inner, CallStatus::Failed(error));
            }
        });
        *lock(&self.inner.cancel) = Some(handle.abort_handle());
    }

    fn abort(&self) {
        if let Some(handle) = lock(&self.inner.cancel).take() {
            handle.abort();
        }
    }

    fn save_flag(&self, enabled: bool) -> std::io::Result<()> {
        if let Some(parent) = self.inner.flag_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.inner.flag_path, format!("{{\"enabled\":{enabled}}}"))
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn set_status(inner: &Inner, status: CallStatus) {
    *lock(&inner.status) = status;
}

async fn run_loop(inner: Arc<Inner>) -> Result<(), String> {
    let endpoint = std::env::var("CALL_EDGE_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    let pow = std::env::var("CALL_POW_DIFFICULTY_BITS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_POW_BITS);

    set_status(&inner, CallStatus::Provisioning);
    let store = match IdentityStore::default_store() {
        Ok(store) => store,
        Err(e) if e.is_disabled() => {
            set_status(&inner, CallStatus::Disabled);
            return Ok(());
        }
        Err(e) => return Err(e.to_string()),
    };
    let identity = match store.load() {
        Ok(Some(id)) => id,
        Ok(None) => provision_new(&endpoint, pow, &store).await?,
        Err(e) if e.is_disabled() => {
            set_status(&inner, CallStatus::Disabled);
            return Ok(());
        }
        Err(e) => return Err(e.to_string()),
    };

    set_status(&inner, CallStatus::Connecting);
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    set_status(&inner, CallStatus::Active);
    match run_agent(AgentConfig {
        endpoint_url: endpoint,
        identity: Arc::new(identity),
        http,
        heartbeat_interval_ms: 5000,
    })
    .await
    {
        Ok(()) => Ok(()),
        Err(e) if e.is_disabled() => {
            set_status(&inner, CallStatus::Disabled);
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

async fn provision_new(
    endpoint: &str,
    pow: u32,
    store: &IdentityStore,
) -> Result<Identity, String> {
    let identity = provision(
        endpoint,
        ProvisionInput {
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            platform: std::env::consts::OS.to_owned(),
            pow_difficulty_bits: pow,
        },
    )
    .await
    .map_err(|e| e.to_string())?;
    store.save(&identity).map_err(|e| e.to_string())?;
    Ok(identity)
}
