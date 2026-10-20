//! One-shot import of the pre-multi-account session files.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use tauri::Manager;

use crate::app::diagnostics::log_native;
use crate::direct::{DirectState, sc};
use crate::rt::AppHandle;

use super::{SESSION_FILE, SessionStore, is_usable};

/// Single-account session written by older builds (migration source).
const LEGACY_SESSION_FILE: &str = "auth_session.json";
/// Zustand-persist file from the upstream app (fallback migration source).
const LEGACY_FILE: &str = "sc-auth.json";
const DIRECT_STORE_FILE: &str = "direct_store.json";
const OFFLINE_INDEX_FILE: &str = "offline-index.json";

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
    let account = match super::account_from_me(&me, token, premium) {
        Ok(account) => account,
        Err(e) => {
            log_native(app, "WARN", format!("[auth] legacy {e}"));
            return;
        }
    };
    let id = account.id;
    match store.seed_legacy(account).await {
        Ok(true) => {
            direct.rebind_owner(Some(id)).await;
            remove_legacy_source(app, &source_path);
        }
        Ok(false) => {
            log_native(
                app,
                "INFO",
                "[auth] session changed during migration; keeping current state",
            );
            // The account list already exists, so the legacy file is
            // superseded. Drop it so signing out of every account later
            // cannot resurrect this session.
            remove_legacy_source(app, &source_path);
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

/// Remove a legacy session file that has been imported or superseded, so a
/// later full sign-out cannot re-import it on the next launch.
fn remove_legacy_source(app: &AppHandle, path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => log_native(
            app,
            "INFO",
            format!("[auth] removed migrated legacy file {}", path.display()),
        ),
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => log_native(
            app,
            "WARN",
            format!(
                "[auth] remove migrated legacy file {} failed: {e}",
                path.display()
            ),
        ),
        Err(_) => {}
    }
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
