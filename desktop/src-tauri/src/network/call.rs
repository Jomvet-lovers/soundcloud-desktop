use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use call_client::{run_agent, AgentConfig, Identity, IdentityStore, ProvisionInput};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex;
use tracing::{info, warn};

const FLAG_FILE: &str = "call_enabled.json";

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
    status: Mutex<CallStatus>,
    runtime: tokio::runtime::Handle,
    cancel: Mutex<Option<tokio::task::AbortHandle>>,
}

impl CallState {
    pub fn init(app_data_dir: PathBuf, runtime: tokio::runtime::Handle) -> Arc<Self> {
        Arc::new(Self {
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
    {
        let mut cancel = state.cancel.lock().await;
        if let Some(h) = cancel.take() {
            h.abort();
        }
    }
    let s = state.clone();
    let handle = tokio::spawn(async move {
        match run_call_loop(app.clone(), s.clone()).await {
            Ok(()) => {}
            Err(e) => {
                warn!(error = %e, "call agent terminated");
                *s.status.lock().await = CallStatus::Failed { error: e };
            }
        }
    });
    *state.cancel.lock().await = Some(handle.abort_handle());
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

async fn run_call_loop(_app: AppHandle, state: Arc<CallState>) -> Result<(), String> {
    let endpoint_url = std::env::var("CALL_EDGE_ENDPOINT")
        .unwrap_or_else(|_| "https://call.scdinternal.site".to_string());
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

    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| fmt_chain(&e))?;

    *state.status.lock().await = CallStatus::Active;
    info!("call agent active");
    match run_agent(AgentConfig {
        endpoint_url,
        identity: Arc::new(identity),
        http,
        heartbeat_interval_ms: 5000,
    })
    .await
    {
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
