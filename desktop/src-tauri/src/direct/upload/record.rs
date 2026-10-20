//! Best-effort local record of uploaded tracks, scoped to the active account.

use std::sync::Arc;

use serde_json::{json, Value};
use tauri::Manager;

use crate::app::diagnostics::log_native;
use crate::direct::DirectState;
use crate::rt::AppHandle;

use super::meta::UploadedTrack;

/// The raw response body when the writer channel preserved a complete Track
/// object. Truncated payloads fall back to the regex-parsed id.
pub(super) fn uploaded_track_object(payload: &str) -> Option<Value> {
    serde_json::from_str::<Value>(payload)
        .ok()
        .filter(|v| v.get("id").and_then(Value::as_u64).is_some())
}

/// Store entry for a finished upload. The raw Track is preferred so the list
/// keeps artwork and user data; otherwise the parsed fields are enough for the
/// `/me/tracks` overlay.
fn recorded_uploaded_track(track: &UploadedTrack, raw: Option<Value>) -> Value {
    match raw.filter(|v| v.get("id").and_then(Value::as_u64).is_some()) {
        Some(raw) => raw,
        None => json!({
            "id": track.id,
            "urn": track.urn,
            "permalink_url": track.permalink_url,
            "title": track.title,
            "state": track.state,
            "uploaded_by_app": true,
        }),
    }
}

/// Best-effort local record, written to the active account's store. It returns
/// `()` on purpose: a missing owner or a store problem must never turn a
/// successful upload into an error.
pub(super) async fn remember_uploaded_track(
    app: &AppHandle,
    token: &str,
    track: &UploadedTrack,
    raw: Option<Value>,
) {
    let direct = app.state::<Arc<DirectState>>().inner().clone();
    let Some(owner) = direct.resolve_owner(Some(token)).await else {
        log_native(
            app,
            "WARN",
            "[upload] account could not be resolved; track not recorded locally",
        );
        return;
    };
    let store = direct.store_for(Some(owner)).await;
    let mut store = store.lock().await;
    store.record_uploaded_track(recorded_uploaded_track(track, raw));
    store.save();
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;

    fn parsed_track(id: u64) -> UploadedTrack {
        UploadedTrack {
            id,
            urn: format!("soundcloud:tracks:{id}"),
            permalink_url: format!("https://soundcloud.com/me/track-{id}"),
            title: "Track".into(),
            state: "processing".into(),
            artwork_error: None,
        }
    }

    #[test]
    fn raw_upload_payload_is_kept_only_for_a_complete_track() {
        assert!(uploaded_track_object(r#"{"id":1,"title":"x"}"#).is_some());
        assert!(uploaded_track_object(r#"{"id":1,"title":"cut off"#).is_none());
        assert!(uploaded_track_object(r#"{"error":"bad request"}"#).is_none());
    }

    #[test]
    fn recorded_upload_prefers_the_raw_track_object() {
        let raw = json!({
            "id": 42,
            "title": "raw",
            "artwork_url": "https://example.com/art.jpg",
        });
        let value = recorded_uploaded_track(&parsed_track(42), Some(raw));

        assert_eq!(
            value.get("artwork_url").and_then(Value::as_str),
            Some("https://example.com/art.jpg"),
        );
        let mut store = crate::direct::store::LocalStore::default();
        store.record_uploaded_track(value);
        assert_eq!(store.uploaded_tracks.len(), 1);
    }

    /// A truncated or non-object body must fall back to the parsed fields
    /// instead of failing the already-successful upload.
    #[test]
    fn recorded_upload_falls_back_when_the_raw_body_is_unusable() {
        for raw in [
            None,
            Some(json!(["not", "an", "object"])),
            Some(json!({ "title": "no id" })),
        ] {
            let value = recorded_uploaded_track(&parsed_track(7), raw);
            assert_eq!(value.get("id").and_then(Value::as_u64), Some(7));
            assert_eq!(
                value.get("urn").and_then(Value::as_str),
                Some("soundcloud:tracks:7"),
            );
            assert_eq!(
                value.get("uploaded_by_app").and_then(Value::as_bool),
                Some(true),
            );
        }
    }
}
