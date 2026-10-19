//! Multi-account SoundCloud sessions: the single source of truth for tokens.
//!
//! Accounts are keyed by SoundCloud user id and persisted together in
//! `auth_sessions.json`. `active` selects the account whose token is mirrored to
//! the frontend (`auth:changed` / `auth_status`), so existing consumers keep
//! working unchanged. The legacy single-account files are imported once at
//! startup (see [`migrate_legacy`]). A source consumed by a successful migration
//! is deleted; on failure it stays in place so the next launch can retry.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{Emitter, Manager, State};
use tokio::sync::RwLock;

use crate::app::diagnostics::log_native;
use crate::direct::{DirectState, sc};
use crate::rt::AppHandle;

const SESSION_FILE: &str = "auth_sessions.json";
/// Single-account session written by older builds (migration source).
const LEGACY_SESSION_FILE: &str = "auth_session.json";
/// Zustand-persist file from the upstream app (fallback migration source).
const LEGACY_FILE: &str = "sc-auth.json";
const EVENT: &str = "auth:changed";
const VERSION: u32 = 2;
const DIRECT_STORE_FILE: &str = "direct_store.json";
const OFFLINE_INDEX_FILE: &str = "offline-index.json";

#[derive(Clone, Serialize, Deserialize)]
pub struct Account {
    id: u64,
    token: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    avatar_url: String,
    #[serde(default)]
    premium: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SessionState {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    accounts: Vec<Account>,
    #[serde(default)]
    active: Option<u64>,
}

fn default_version() -> u32 {
    VERSION
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            version: VERSION,
            accounts: Vec::new(),
            active: None,
        }
    }
}

/// Wire shape for `auth_status` and `auth:changed`. `token` / `premium` mirror
/// the active account so the existing single-session frontend keeps working.
#[derive(Clone, Serialize)]
pub struct AuthView {
    accounts: Vec<Account>,
    active: Option<u64>,
    token: Option<String>,
    premium: bool,
}

impl SessionState {
    fn view(&self) -> AuthView {
        let active = self
            .active
            .and_then(|id| self.accounts.iter().find(|a| a.id == id));
        AuthView {
            accounts: self.accounts.clone(),
            active: self.active,
            token: active.map(|a| a.token.clone()),
            premium: active.is_some_and(|a| a.premium),
        }
    }
}

pub struct SessionStore {
    path: PathBuf,
    state: RwLock<SessionState>,
    http: wreq::Client,
    rt: tokio::runtime::Handle,
}

impl SessionStore {
    pub fn init(
        app_data_dir: PathBuf,
        http: wreq::Client,
        rt: tokio::runtime::Handle,
    ) -> Arc<Self> {
        let path = app_data_dir.join(SESSION_FILE);
        let state = load_state(&path).unwrap_or_default();
        Arc::new(Self {
            path,
            state: RwLock::new(state),
            http,
            rt,
        })
    }

    /// Programmatic session set (direct-mode token login). Resolves the account
    /// via `/me`, then upserts and activates it exactly like `auth_set_session`.
    pub async fn set_token(&self, app: &AppHandle, token: String) -> Result<(), String> {
        let direct = app.state::<Arc<DirectState>>().inner().clone();
        let me = sc::fetch_me(direct.as_ref(), &token).await?;
        let id = me
            .get("id")
            .and_then(Value::as_u64)
            .ok_or_else(|| "SoundCloud /me returned no user id".to_string())?;
        let username = me
            .get("username")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let avatar_url = me
            .get("avatar_url")
            .and_then(Value::as_str)
            .unwrap_or_default();
        self.upsert_account(app, token, id, username, avatar_url)
            .await
    }

    /// Add a new account or refresh an existing one and make it active. Premium
    /// starts at `false`; the `/me/subscription` recheck restores it.
    pub async fn upsert_account(
        &self,
        app: &AppHandle,
        token: String,
        me_id: u64,
        username: &str,
        avatar_url: &str,
    ) -> Result<(), String> {
        let mut guard = self.state.write().await;
        match guard.accounts.iter_mut().find(|a| a.id == me_id) {
            Some(account) => {
                account.token = token;
                account.username = username.to_string();
                account.avatar_url = avatar_url.to_string();
                account.premium = false;
            }
            None => guard.accounts.push(Account {
                id: me_id,
                token,
                username: username.to_string(),
                avatar_url: avatar_url.to_string(),
                premium: false,
            }),
        }
        guard.active = Some(me_id);
        bind_active(app, Some(me_id)).await;
        persist_and_emit(app, &self.path, &guard, "session");
        Ok(())
    }

    pub async fn switch_account(&self, app: &AppHandle, id: u64) -> Result<(), String> {
        let mut guard = self.state.write().await;
        if !guard.accounts.iter().any(|a| a.id == id) {
            return Err(format!("account {id} not found"));
        }
        if guard.active == Some(id) {
            bind_active(app, Some(id)).await;
            return Ok(());
        }
        guard.active = Some(id);
        bind_active(app, Some(id)).await;
        persist_and_emit(app, &self.path, &guard, "switch");
        Ok(())
    }

    pub async fn remove_account(&self, app: &AppHandle, id: u64) -> Result<(), String> {
        let mut guard = self.state.write().await;
        let Some(pos) = guard.accounts.iter().position(|a| a.id == id) else {
            return Err(format!("account {id} not found"));
        };
        guard.accounts.remove(pos);
        if guard.active == Some(id) {
            guard.active = guard.accounts.first().map(|a| a.id);
        }
        bind_active(app, guard.active).await;
        persist_and_emit(app, &self.path, &guard, "remove");
        Ok(())
    }

    pub async fn set_premium(&self, app: &AppHandle, premium: bool) -> Result<(), String> {
        let mut guard = self.state.write().await;
        let Some(id) = guard.active else {
            return Ok(());
        };
        let Some(account) = guard.accounts.iter_mut().find(|a| a.id == id) else {
            return Ok(());
        };
        if account.premium == premium {
            return Ok(());
        }
        account.premium = premium;
        persist_and_emit(app, &self.path, &guard, "premium");
        Ok(())
    }

    /// Sign out of the active account only. Remaining accounts stay signed in;
    /// the token of the removed account is revoked server-side in the
    /// background.
    pub async fn logout(&self, app: &AppHandle, api_base: &str) -> Result<(), String> {
        let removed = {
            let mut guard = self.state.write().await;
            let active = guard.active;
            let removed = active.and_then(|id| {
                guard
                    .accounts
                    .iter()
                    .position(|a| a.id == id)
                    .map(|pos| guard.accounts.remove(pos))
            });
            if active.is_some() {
                guard.active = guard.accounts.first().map(|a| a.id);
            }
            bind_active(app, guard.active).await;
            persist_and_emit(app, &self.path, &guard, "logout");
            removed
        };

        if let Some(account) = removed {
            let http = self.http.clone();
            let app = app.clone();
            let api_base = api_base.to_string();
            self.rt.spawn(async move {
                let url = format!("{}/auth/logout", api_base.trim_end_matches('/'));
                match http
                    .post(url)
                    .header("x-session-id", account.token)
                    .timeout(Duration::from_secs(10))
                    .send()
                    .await
                {
                    Ok(r) => {
                        log_native(&app, "INFO", format!("[auth] server logout {}", r.status()))
                    }
                    Err(e) => log_native(&app, "WARN", format!("[auth] server logout failed: {e}")),
                }
            });
        }
        Ok(())
    }

    /// Import a legacy session without emitting; the migration task emits once
    /// after the account-scoped files have been moved too.
    async fn seed_legacy(&self, account: Account) -> std::io::Result<bool> {
        let mut guard = self.state.write().await;
        if !guard.accounts.is_empty() {
            return Ok(false);
        }
        guard.active = Some(account.id);
        guard.accounts.push(account);
        write_state(&self.path, &guard)?;
        Ok(true)
    }

    pub async fn emit_view(&self, app: &AppHandle) {
        // Hold the read lock across bind + emit so a concurrent account change
        // cannot interleave its own rebind/view between the two.
        let state = self.state.read().await;
        bind_active(app, state.active).await;
        app.emit(EVENT, state.view()).ok();
    }

    /// Token of the active account, or `None` when signed out. Used by the
    /// direct commands that write to SoundCloud without a request token.
    pub async fn active_token(&self) -> Option<String> {
        let token = self.state.read().await.view().token;
        token.filter(|t| is_usable(t))
    }
}

fn is_usable(token: &str) -> bool {
    !token.is_empty() && token != "undefined" && token != "null"
}

/// Persist + emit under the caller's write lock so file/event order matches
/// memory. Persistence failures are logged; the in-memory state stays usable.
fn persist_and_emit(app: &AppHandle, path: &Path, state: &SessionState, action: &str) {
    if let Err(e) = write_state(path, state) {
        log_native(app, "ERROR", format!("[auth] {action} persist failed: {e}"));
    }
    app.emit(EVENT, state.view()).ok();
}

/// Point the direct store at the active account before `auth:changed` reaches
/// the frontend, so token-less requests cannot read another account's data.
async fn bind_active(app: &AppHandle, active: Option<u64>) {
    let direct = app.state::<Arc<DirectState>>().inner().clone();
    direct.rebind_owner(active).await;
}

fn load_state(path: &Path) -> Option<SessionState> {
    let bytes = std::fs::read(path).ok()?;
    let mut state = serde_json::from_slice::<SessionState>(&bytes).ok()?;
    state.version = VERSION;
    state.accounts.retain(|a| is_usable(&a.token));
    if !state.accounts.iter().any(|a| Some(a.id) == state.active) {
        state.active = None;
    }
    Some(state)
}

/// Atomic write: tmp -> fsync -> rename. The file is removed only when no
/// accounts remain; an account list with `active == None` is still persisted.
fn write_state(path: &Path, state: &SessionState) -> std::io::Result<()> {
    if state.accounts.is_empty() {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec(state).map_err(std::io::Error::other)?;
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// One-shot background migration from the single-account files. Always emits
/// the resulting session once, even when there is nothing to migrate or the
/// `/me` lookup fails. The consumed legacy file is deleted only after it has
/// been imported successfully; failed attempts keep it for the next launch.
pub async fn migrate_legacy(app: AppHandle, data_dir: PathBuf) {
    let store = app.state::<Arc<SessionStore>>().inner().clone();
    migrate_impl(&store, &app, &data_dir).await;
    store.emit_view(&app).await;
}

async fn migrate_impl(store: &SessionStore, app: &AppHandle, data_dir: &Path) {
    if data_dir.join(SESSION_FILE).exists() {
        log_native(
            app,
            "INFO",
            "[auth] accounts file present; migration skipped",
        );
        return;
    }
    let Some(LegacySource {
        token,
        premium,
        path: source_path,
    }) = legacy_source(data_dir)
    else {
        log_native(app, "INFO", "[auth] no legacy session to migrate");
        return;
    };

    let direct = app.state::<Arc<DirectState>>().inner().clone();
    let me = match sc::fetch_me(direct.as_ref(), &token).await {
        Ok(me) => me,
        Err(e) => {
            log_native(app, "WARN", format!("[auth] legacy /me lookup failed: {e}"));
            return;
        }
    };
    let Some(id) = me.get("id").and_then(Value::as_u64) else {
        log_native(app, "WARN", "[auth] legacy /me returned no user id");
        return;
    };
    let account = Account {
        id,
        token,
        username: me
            .get("username")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        avatar_url: me
            .get("avatar_url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        premium,
    };
    match store.seed_legacy(account).await {
        Ok(true) => {
            direct.rebind_owner(Some(id)).await;
            if let Err(e) = std::fs::remove_file(&source_path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    log_native(
                        app,
                        "WARN",
                        format!(
                            "[auth] remove migrated legacy file {} failed: {e}",
                            source_path.display()
                        ),
                    );
                }
            } else {
                log_native(
                    app,
                    "INFO",
                    format!(
                        "[auth] removed migrated legacy file {}",
                        source_path.display()
                    ),
                );
            }
        }
        Ok(false) => {
            log_native(
                app,
                "INFO",
                "[auth] session changed during migration; keeping current state",
            );
            return;
        }
        Err(e) => {
            log_native(
                app,
                "ERROR",
                format!("[auth] legacy session persist failed: {e}"),
            );
            return;
        }
    }

    rename_if_absent(
        app,
        &data_dir.join(DIRECT_STORE_FILE),
        &data_dir.join(format!("direct_store.{id}.json")),
    );
    rename_if_absent(
        app,
        &data_dir.join(OFFLINE_INDEX_FILE),
        &data_dir.join(format!("offline-index.{id}.json")),
    );
    log_native(
        app,
        "INFO",
        format!("[auth] migrated legacy session for account {id}"),
    );
}

/// A legacy token together with the file it was read from, so a successful
/// migration can delete only the source it actually consumed.
struct LegacySource {
    token: String,
    premium: bool,
    path: PathBuf,
}

/// Token and premium from the legacy single-account file, falling back to the
/// upstream zustand file when the newer one is missing or malformed.
fn legacy_source(data_dir: &Path) -> Option<LegacySource> {
    fn read(path: &Path) -> Option<Value> {
        let bytes = std::fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    let session_path = data_dir.join(LEGACY_SESSION_FILE);
    if let Some(v) = read(&session_path) {
        if let Some(token) = v.get("token").and_then(Value::as_str) {
            if is_usable(token) {
                let premium = v.get("premium").and_then(Value::as_bool).unwrap_or(false);
                return Some(LegacySource {
                    token: token.to_string(),
                    premium,
                    path: session_path,
                });
            }
        }
    }

    let path = data_dir.join(LEGACY_FILE);
    let v = read(&path)?;
    let token = v.get("state")?.get("sessionId")?.as_str()?;
    is_usable(token).then(|| LegacySource {
        token: token.to_string(),
        premium: false,
        path,
    })
}

fn rename_if_absent(app: &AppHandle, from: &Path, to: &Path) {
    if !from.exists() || to.exists() {
        return;
    }
    match std::fs::rename(from, to) {
        Ok(()) => log_native(
            app,
            "INFO",
            format!("[auth] moved {} to {}", from.display(), to.display()),
        ),
        Err(e) => log_native(
            app,
            "WARN",
            format!("[auth] move {} failed: {e}", from.display()),
        ),
    }
}

#[tauri::command]
pub async fn auth_status(state: State<'_, Arc<SessionStore>>) -> Result<AuthView, String> {
    Ok(state.state.read().await.view())
}

#[tauri::command]
pub async fn auth_set_session(
    token: String,
    app: AppHandle,
    state: State<'_, Arc<SessionStore>>,
) -> Result<(), String> {
    state.set_token(&app, token).await
}

#[tauri::command]
pub async fn auth_switch_account(
    id: u64,
    app: AppHandle,
    state: State<'_, Arc<SessionStore>>,
) -> Result<(), String> {
    state.switch_account(&app, id).await
}

#[tauri::command]
pub async fn auth_remove_account(
    id: u64,
    app: AppHandle,
    state: State<'_, Arc<SessionStore>>,
) -> Result<(), String> {
    state.remove_account(&app, id).await
}

#[tauri::command]
pub async fn auth_set_premium(
    premium: bool,
    app: AppHandle,
    state: State<'_, Arc<SessionStore>>,
) -> Result<(), String> {
    state.set_premium(&app, premium).await
}

#[tauri::command]
pub async fn auth_logout(
    api_base: String,
    app: AppHandle,
    state: State<'_, Arc<SessionStore>>,
) -> Result<(), String> {
    state.logout(&app, &api_base).await
}
