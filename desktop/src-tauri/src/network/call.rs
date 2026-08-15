use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::rt::AppHandle;
use call_client::{AgentConfig, Identity, IdentityStore, ProvisionInput, run_agent_session};
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use tokio::sync::Mutex;
use tracing::{info, warn};

const FLAG_FILE: &str = "call_enabled.json";
const DEVICE_FILE: &str = "call_device.json";
const ORIGIN_ZONE: &str = "scnative.space";
const DEFAULT_ENDPOINT: &str = "https://call.scnative.space";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CallStatus {
    Disabled,
    Connecting,
    Provisioning,
    Active,
    Failed { error: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct EnabledFlag {
    enabled: bool,
}

pub struct CallState {
    config_path: PathBuf,
    device_id: String,
    status: Mutex<CallStatus>,
    runtime: tokio::runtime::Handle,
    cancel: Mutex<Option<tokio::task::AbortHandle>>,
}

impl CallState {
    pub fn init(app_data_dir: PathBuf, runtime: tokio::runtime::Handle) -> Arc<Self> {
        Arc::new(Self {
            device_id: load_or_create_device_id(&app_data_dir),
            config_path: app_data_dir.join(FLAG_FILE),
            status: Mutex::new(CallStatus::Disabled),
            runtime,
            cancel: Mutex::new(None),
        })
    }

    fn load_flag(&self) -> bool {
        match std::fs::read(&self.config_path) {
            Ok(b) => serde_json::from_slice::<EnabledFlag>(&b)
                .map(|f| f.enabled)
                .unwrap_or(true),
            Err(_) => true,
        }
    }

    fn save_flag(&self, enabled: bool) -> Result<(), String> {
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let bytes = serde_json::to_vec(&EnabledFlag { enabled }).map_err(|e| e.to_string())?;
        std::fs::write(&self.config_path, bytes).map_err(|e| e.to_string())
    }
}

pub fn maybe_autostart(app: &AppHandle, state: Arc<CallState>) {
    if !state.load_flag() {
        return;
    }
    let app = app.clone();
    let s = state.clone();
    state.runtime.spawn(async move {
        spawn_agent(app, s).await;
    });
}

async fn spawn_agent(app: AppHandle, state: Arc<CallState>) {
    let mut cancel = state.cancel.lock().await;
    if let Some(handle) = cancel.take() {
        handle.abort();
    }
    let s = state.clone();
    let handle = tokio::spawn(async move {
        supervise(app, s).await;
    });
    *cancel = Some(handle.abort_handle());
}

async fn supervise(app: AppHandle, state: Arc<CallState>) {
    let mut backoff = Duration::from_secs(5);
    loop {
        let mut connected = false;

        for endpoint in endpoint_candidates(&state.device_id).await {
            let became_active = Arc::new(AtomicBool::new(false));
            let result =
                run_call_loop(app.clone(), state.clone(), &endpoint, became_active.clone()).await;
            if became_active.load(Ordering::Relaxed) {
                connected = true;
            }
            match result {
                Ok(()) => {
                    break;
                }
                Err(e) => {
                    warn!(endpoint = %endpoint, error = %e, "call agent terminated");
                    *state.status.lock().await = CallStatus::Failed { error: e };
                }
            }
        }

        if matches!(*state.status.lock().await, CallStatus::Disabled) {
            return;
        }
        backoff = if connected {
            Duration::from_secs(5)
        } else {
            (backoff * 2).min(Duration::from_secs(300))
        };
        tokio::time::sleep(backoff).await;
    }
}

async fn endpoint_candidates(device_id: &str) -> Vec<String> {
    if let Ok(configured) = std::env::var("CALL_EDGE_ENDPOINT") {
        return vec![configured];
    }
    let pool = crate::network::edge::call_pool();
    if pool.is_empty() {
        return vec![DEFAULT_ENDPOINT.to_string()];
    }

    let ordered = super::call_nodes::order(device_id, &pool);
    let http = match sc_fingerprint::builder(None)
        .connect_timeout(Duration::from_secs(5))
        .build()
    {
        Ok(http) => http,
        Err(_) => return vec![DEFAULT_ENDPOINT.to_string()],
    };

    let mut usable = Vec::new();
    for node in ordered {
        let endpoint = format!("https://{node}.{ORIGIN_ZONE}");
        let reach = super::call_nodes::reach(&http, &endpoint).await;
        if reach.usable() {
            usable.push(endpoint);
        } else {
            warn!(node = %node, reach = reach.as_str(), "call node path unusable, skipping");
        }
    }
    if usable.is_empty() {
        vec![DEFAULT_ENDPOINT.to_string()]
    } else {
        usable
    }
}

fn load_or_create_device_id(dir: &std::path::Path) -> String {
    let path = dir.join(DEVICE_FILE);
    if let Ok(raw) = std::fs::read(&path)
        && let Ok(stored) = serde_json::from_slice::<DeviceId>(&raw)
    {
        let id = stored.device_id.trim();
        if !id.is_empty() && id.len() <= 64 {
            return id.to_string();
        }
    }
    let device_id = uuid::Uuid::new_v4().to_string();
    if let Ok(raw) = serde_json::to_vec(&DeviceId {
        device_id: device_id.clone(),
    }) {
        let temp = path.with_extension("tmp");
        if std::fs::write(&temp, raw).is_ok() && std::fs::rename(&temp, &path).is_err() {
            let _ = std::fs::remove_file(temp);
        }
    }
    device_id
}

#[derive(Serialize, Deserialize)]
struct DeviceId {
    device_id: String,
}

fn fmt_chain<E: std::error::Error + ?Sized>(e: &E) -> String {
    let mut out = e.to_string();
    let mut src = e.source();
    while let Some(s) = src {
        out.push_str(" | ");
        out.push_str(&s.to_string());
        src = s.source();
    }
    out
}

async fn run_call_loop(
    _app: AppHandle,
    state: Arc<CallState>,
    endpoint: &str,
    became_active: Arc<AtomicBool>,
) -> Result<(), String> {
    let endpoint_url = endpoint.to_string();
    let pow_difficulty = std::env::var("CALL_POW_DIFFICULTY_BITS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(22u32);

    *state.status.lock().await = CallStatus::Provisioning;
    let store = match IdentityStore::default_store() {
        Ok(s) => s,
        Err(e) if e.is_disabled() => {
            *state.status.lock().await = CallStatus::Disabled;
            return Ok(());
        }
        Err(e) => return Err(fmt_chain(&e)),
    };
    let identity = match store.load() {
        Ok(Some(id)) => id,
        Ok(None) => provision_new(&endpoint_url, pow_difficulty, &state).await?,
        Err(e) if e.is_disabled() => {
            *state.status.lock().await = CallStatus::Disabled;
            return Ok(());
        }
        Err(e) => return Err(fmt_chain(&e)),
    };

    *state.status.lock().await = CallStatus::Connecting;

    let http = sc_fingerprint::builder(None)
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| fmt_chain(&e))?;

    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let session = run_agent_session(
        AgentConfig {
            endpoint_url,
            identity: Arc::new(identity),
            http,
            heartbeat_interval_ms: 5000,
        },
        move || {
            let _ = ready_tx.send(());
        },
    );
    tokio::pin!(session);
    let result = tokio::select! {
        biased;
        ready = ready_rx => {
            if ready.is_ok() {
                became_active.store(true, Ordering::Relaxed);
                *state.status.lock().await = CallStatus::Active;
                info!("call agent active");
            }
            session.await
        },
        result = &mut session => result,
    };
    match result {
        Ok(()) => Ok(()),
        Err(e) if e.is_disabled() => {
            *state.status.lock().await = CallStatus::Disabled;
            Ok(())
        }
        Err(e) => Err(fmt_chain(&e)),
    }
}

async fn provision_new(
    endpoint_url: &str,
    pow_difficulty: u32,
    state: &Arc<CallState>,
) -> Result<Identity, String> {
    let id = match call_client::provision(
        endpoint_url,
        ProvisionInput {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            platform: std::env::consts::OS.to_string(),
            pow_difficulty_bits: pow_difficulty,
        },
    )
    .await
    {
        Ok(id) => id,
        Err(e) if e.is_disabled() => {
            *state.status.lock().await = CallStatus::Disabled;
            return Err("disabled".to_string());
        }
        Err(e) => return Err(fmt_chain(&e)),
    };
    let store = IdentityStore::default_store().map_err(|e| fmt_chain(&e))?;
    store.save(&id).map_err(|e| fmt_chain(&e))?;
    Ok(id)
}

#[tauri::command]
pub async fn call_set_enabled(
    enabled: bool,
    app: AppHandle,
    state: State<'_, Arc<CallState>>,
) -> Result<CallStatus, String> {
    let s = state.inner().clone();
    s.save_flag(enabled)?;
    if enabled {
        spawn_agent(app, s.clone()).await;
        Ok(s.status.lock().await.clone())
    } else {
        let mut cancel = s.cancel.lock().await;
        if let Some(h) = cancel.take() {
            h.abort();
        }
        *s.status.lock().await = CallStatus::Disabled;
        Ok(CallStatus::Disabled)
    }
}

#[tauri::command]
pub fn call_is_enabled(state: State<'_, Arc<CallState>>) -> bool {
    state.inner().load_flag()
}

#[tauri::command]
pub async fn call_status(state: State<'_, Arc<CallState>>) -> Result<CallStatus, String> {
    Ok(state.inner().status.lock().await.clone())
}

pub fn manage_state(app: &AppHandle, state: Arc<CallState>) {
    app.manage(state);
}

#[cfg(test)]
mod tests {
    use super::DEFAULT_ENDPOINT;

    #[test]
    fn default_edge_endpoint_uses_standard_https_port() {
        let endpoint = url::Url::parse(DEFAULT_ENDPOINT).expect("valid default Edge URL");

        assert_eq!(endpoint.port(), None);
        assert_eq!(endpoint.port_or_known_default(), Some(443));
    }
}
