//! Local persistent store for direct-mode user actions.
//!
//! In direct mode the app talks to SoundCloud read-only. Anything that would
//! write to the SC account (likes, follows, playlist edits, comments, history)
//! is persisted locally and overlaid on top of the SC read results.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

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
}
