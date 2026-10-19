//! Local persistent store for direct-mode user actions.
//!
//! In direct mode the app talks to SoundCloud read-only. Anything that would
//! write to the SC account (likes, follows, playlist edits, comments, history)
//! is persisted locally and overlaid on top of the SC read results.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Enough to cover a long upload session; anything older has long since been
/// returned by SoundCloud and pruned.
const MAX_UPLOADED_TRACKS: usize = 50;

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct LocalStore {
    /// Tracks liked locally (full track objects, newest first).
    #[serde(default)]
    pub liked_tracks: Vec<Value>,
    /// URNs that were unliked locally (SC like exists but must be hidden).
    #[serde(default)]
    pub unliked_tracks: Vec<String>,
    /// Playlists liked locally (full playlist objects, newest first).
    #[serde(default)]
    pub liked_playlists: Vec<Value>,
    /// Playlist URNs unliked locally.
    #[serde(default)]
    pub unliked_playlists: Vec<String>,
    /// User URNs followed locally.
    #[serde(default)]
    pub followed: Vec<String>,
    /// User URNs unfollowed locally.
    #[serde(default)]
    pub unfollowed: Vec<String>,
    /// Playlists created/edited in the app (full playlist objects).
    #[serde(default)]
    pub playlists: Vec<Value>,
    /// Deleted playlist URNs (hide SC-owned ones locally).
    #[serde(default)]
    pub deleted_playlists: Vec<String>,
    /// Playback history (newest first).
    #[serde(default)]
    pub history: Vec<Value>,
    /// Locally disliked track URNs.
    #[serde(default)]
    pub disliked: Vec<String>,
    /// Comments posted locally: {urn, body, timestamp, created_at}.
    #[serde(default)]
    pub comments: Vec<Value>,
    /// Tracks uploaded from this app, newest first. Each entry is a SoundCloud
    /// track object plus `"uploaded_by_app": true`.
    #[serde(default)]
    pub uploaded_tracks: Vec<Value>,

    #[serde(skip)]
    pub path: Option<PathBuf>,
}

impl LocalStore {
    pub fn load(path: &Path) -> Self {
        let mut store: Self = std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        store.path = Some(path.to_path_buf());
        store
    }

    pub fn save(&self) {
        let Some(path) = &self.path else { return };
        let bytes = match serde_json::to_vec_pretty(self) {
            Ok(b) => b,
            Err(_) => return,
        };
        let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
        if std::fs::write(&tmp, &bytes).is_ok() && std::fs::rename(&tmp, path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    pub fn is_unliked(&self, urn: &str) -> bool {
        self.unliked_tracks.iter().any(|u| u == urn)
    }

    pub fn is_liked(&self, urn: &str) -> bool {
        self.liked_tracks
            .iter()
            .any(|t| t.get("urn").and_then(Value::as_str) == Some(urn))
    }

    pub fn like_track(&mut self, track: Value) {
        let urn = track
            .get("urn")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        self.unliked_tracks.retain(|u| u != &urn);
        self.liked_tracks.retain(|t| {
            t.get("urn").and_then(Value::as_str) != Some(urn.as_str())
        });
        self.liked_tracks.insert(0, track);
        self.save();
    }

    pub fn unlike_track(&mut self, urn: &str) {
        self.liked_tracks.retain(|t| {
            t.get("urn").and_then(Value::as_str) != Some(urn)
        });
        if !self.unliked_tracks.iter().any(|u| u == urn) {
            self.unliked_tracks.push(urn.to_string());
        }
        self.save();
    }

    pub fn is_playlist_unliked(&self, urn: &str) -> bool {
        self.unliked_playlists.iter().any(|u| u == urn)
    }

    pub fn is_playlist_liked(&self, urn: &str) -> bool {
        self.liked_playlists
            .iter()
            .any(|p| p.get("urn").and_then(Value::as_str) == Some(urn))
    }

    pub fn like_playlist(&mut self, playlist: Value) {
        let urn = playlist
            .get("urn")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        self.unliked_playlists.retain(|u| u != &urn);
        self.liked_playlists
            .retain(|p| p.get("urn").and_then(Value::as_str) != Some(urn.as_str()));
        self.liked_playlists.insert(0, playlist);
        self.save();
    }

    pub fn unlike_playlist(&mut self, urn: &str) {
        self.liked_playlists
            .retain(|p| p.get("urn").and_then(Value::as_str) != Some(urn));
        if !self.unliked_playlists.iter().any(|u| u == urn) {
            self.unliked_playlists.push(urn.to_string());
        }
        self.save();
    }

    pub fn find_playlist(&self, urn: &str) -> Option<Value> {
        self.playlists
            .iter()
            .find(|p| p.get("urn").and_then(Value::as_str) == Some(urn))
            .cloned()
    }

    pub fn upsert_playlist(&mut self, playlist: Value) {
        let urn = playlist
            .get("urn")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        self.deleted_playlists.retain(|u| u != &urn);
        self.playlists
            .retain(|p| p.get("urn").and_then(Value::as_str) != Some(urn.as_str()));
        self.playlists.insert(0, playlist);
        self.save();
    }

    pub fn rekey_playlist(&mut self, old_urn: &str, playlist: Value) {
        self.playlists
            .retain(|p| p.get("urn").and_then(Value::as_str) != Some(old_urn));
        self.upsert_playlist(playlist);
    }

    /// Mirror SoundCloud's privacy onto a local copy of an SC-owned set.
    /// SoundCloud owns `sharing` for its own sets, so a stale local value must
    /// not win; returns true when the stored value changed (the caller saves).
    pub fn set_playlist_sharing(&mut self, urn: &str, sharing: &str) -> bool {
        let Some(p) = self
            .playlists
            .iter_mut()
            .find(|p| p.get("urn").and_then(Value::as_str) == Some(urn))
        else {
            return false;
        };
        if p.get("sharing").and_then(Value::as_str) == Some(sharing) {
            return false;
        }
        if let Some(obj) = p.as_object_mut() {
            obj.insert("sharing".into(), serde_json::json!(sharing));
        }
        true
    }

    pub fn delete_playlist(&mut self, urn: &str) {
        self.playlists
            .retain(|p| p.get("urn").and_then(Value::as_str) != Some(urn));
        if !self.deleted_playlists.iter().any(|u| u == urn) {
            self.deleted_playlists.push(urn.to_string());
        }
        self.save();
    }

    pub fn is_deleted_playlist(&self, urn: &str) -> bool {
        self.deleted_playlists.iter().any(|u| u == urn)
    }

    pub fn next_local_playlist_urn(&self) -> String {
        let max = self
            .playlists
            .iter()
            .filter_map(|p| p.get("urn").and_then(Value::as_str))
            .filter_map(|u| u.strip_prefix("local:playlists:"))
            .filter_map(|n| n.parse::<u64>().ok())
            .max()
            .unwrap_or(0);
        format!("local:playlists:{}", max + 1)
    }

    /// Record a freshly uploaded track. Newest first, de-duplicated by id.
    pub fn record_uploaded_track(&mut self, mut track: Value) {
        if let Some(obj) = track.as_object_mut() {
            obj.insert("uploaded_by_app".into(), Value::Bool(true));
        }
        if let Some(id) = track.get("id").and_then(Value::as_u64) {
            self.uploaded_tracks
                .retain(|t| t.get("id").and_then(Value::as_u64) != Some(id));
        }
        self.uploaded_tracks.insert(0, track);
        self.uploaded_tracks.truncate(MAX_UPLOADED_TRACKS);
    }

    /// Uploaded tracks that are not in the given set of SoundCloud ids.
    pub fn pending_uploaded_tracks(&self, known_ids: &HashSet<u64>) -> Vec<Value> {
        self.uploaded_tracks
            .iter()
            .filter(|t| {
                t.get("id")
                    .and_then(Value::as_u64)
                    .map_or(true, |id| !known_ids.contains(&id))
            })
            .cloned()
            .collect()
    }

    /// Drop entries that SoundCloud now returns.
    pub fn prune_uploaded_tracks(&mut self, known_ids: &HashSet<u64>) {
        self.uploaded_tracks.retain(|t| {
            t.get("id")
                .and_then(Value::as_u64)
                .map_or(true, |id| !known_ids.contains(&id))
        });
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{LocalStore, MAX_UPLOADED_TRACKS};
    use serde_json::json;

    fn uploaded(id: u64, title: &str) -> serde_json::Value {
        json!({ "id": id, "kind": "track", "title": title })
    }

    /// SC-owned copies follow SoundCloud's privacy; only real changes report
    /// true so the callers can batch a single store save.
    #[test]
    fn sharing_follows_soundcloud() {
        let mut store = LocalStore::default();
        store.playlists.push(json!({
            "urn": "soundcloud:playlists:1",
            "sharing": "public",
        }));

        assert!(store.set_playlist_sharing("soundcloud:playlists:1", "private"));
        assert_eq!(
            store.playlists[0].get("sharing").and_then(|v| v.as_str()),
            Some("private"),
        );
        assert!(!store.set_playlist_sharing("soundcloud:playlists:1", "private"));
        assert!(!store.set_playlist_sharing("soundcloud:playlists:2", "private"));
    }

    #[test]
    fn recorded_uploads_go_to_the_front_and_are_tagged() {
        let mut store = LocalStore::default();
        store.record_uploaded_track(uploaded(1, "first"));
        store.record_uploaded_track(uploaded(2, "second"));

        let ids: Vec<u64> = store
            .uploaded_tracks
            .iter()
            .filter_map(|t| t.get("id").and_then(|v| v.as_u64()))
            .collect();
        assert_eq!(ids, [2, 1]);
        assert_eq!(
            store.uploaded_tracks[0]
                .get("uploaded_by_app")
                .and_then(|v| v.as_bool()),
            Some(true),
        );
    }

    #[test]
    fn recording_the_same_id_replaces_and_moves_it_to_the_front() {
        let mut store = LocalStore::default();
        store.record_uploaded_track(uploaded(1, "old"));
        store.record_uploaded_track(uploaded(2, "other"));
        store.record_uploaded_track(uploaded(1, "new"));

        assert_eq!(store.uploaded_tracks.len(), 2);
        assert_eq!(
            store.uploaded_tracks[0].get("id").and_then(|v| v.as_u64()),
            Some(1),
        );
        assert_eq!(
            store.uploaded_tracks[0]
                .get("title")
                .and_then(|v| v.as_str()),
            Some("new"),
        );
        assert_eq!(
            store.uploaded_tracks[1].get("id").and_then(|v| v.as_u64()),
            Some(2),
        );
    }

    #[test]
    fn recorded_uploads_are_capped_with_the_oldest_dropped() {
        let mut store = LocalStore::default();
        for id in 0..=(MAX_UPLOADED_TRACKS as u64) {
            store.record_uploaded_track(uploaded(id, "t"));
        }

        assert_eq!(store.uploaded_tracks.len(), MAX_UPLOADED_TRACKS);
        assert_eq!(
            store.uploaded_tracks[0].get("id").and_then(|v| v.as_u64()),
            Some(MAX_UPLOADED_TRACKS as u64),
        );
        assert!(store.uploaded_tracks.iter().all(|t| {
            t.get("id").and_then(|v| v.as_u64()) != Some(0)
        }));
    }

    #[test]
    fn pending_uploads_skip_ids_soundcloud_already_returns() {
        let mut store = LocalStore::default();
        store.record_uploaded_track(uploaded(1, "known"));
        store.record_uploaded_track(uploaded(2, "pending"));
        store.uploaded_tracks.push(json!({ "title": "no id" }));

        let known: HashSet<u64> = [1].into_iter().collect();
        let pending = store.pending_uploaded_tracks(&known);
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].get("id").and_then(|v| v.as_u64()), Some(2));
        // An entry without an id cannot be matched and must survive.
        assert!(pending[1].get("id").is_none());
    }

    #[test]
    fn prune_drops_ids_soundcloud_now_returns() {
        let mut store = LocalStore::default();
        store.record_uploaded_track(uploaded(1, "known"));
        store.record_uploaded_track(uploaded(2, "pending"));
        store.uploaded_tracks.push(json!({ "title": "no id" }));

        let known: HashSet<u64> = [1].into_iter().collect();
        store.prune_uploaded_tracks(&known);

        assert_eq!(store.uploaded_tracks.len(), 2);
        assert!(store.uploaded_tracks.iter().all(|t| {
            t.get("id").and_then(|v| v.as_u64()) != Some(1)
        }));
    }

    #[test]
    fn old_store_json_without_uploaded_tracks_still_loads() {
        let store: LocalStore = serde_json::from_value(json!({
            "liked_tracks": [],
            "comments": [],
        }))
        .expect("missing fields must fall back to defaults");

        assert!(store.uploaded_tracks.is_empty());
    }
}
