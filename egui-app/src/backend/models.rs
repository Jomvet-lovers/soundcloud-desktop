//! Phase 3: ローカル API の応答型 (`direct/routes/normalize` 出力に対応)。
//! 対応: `desktop/src/stores/player.ts` の `Track` 他。JSON は余剰フィールドを
//! 無視し、欠損は既定値で埋める。タイトル/アーティストの高度な分解
//! (`lib/track-display`) は簡略版に留める (Phase 3 以降で移植)。

use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ScUser {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub urn: String,
    #[serde(default)]
    pub username: String,
    pub avatar_url: Option<String>,
    pub permalink_url: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Track {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub urn: String,
    #[serde(default)]
    pub title: String,
    /// ミリ秒 (React の `dur()` と同じ)。
    #[serde(default)]
    pub duration: i64,
    pub artwork_url: Option<String>,
    pub permalink_url: Option<String>,
    pub waveform_url: Option<String>,
    pub genre: Option<String>,
    pub playback_count: Option<i64>,
    pub likes_count: Option<i64>,
    pub comment_count: Option<i64>,
    pub user: Option<ScUser>,
    /// API が返す「自分がいいね済みか」(api-v2、認証時のみ)。
    pub user_favorite: Option<bool>,
    /// アップロード日時 (ISO8601。Fresh drops の並び替え用)。
    pub created_at: Option<String>,
}

/// `GET /likes/{tracks|playlists}/:urn` の応答。
#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct LikedFlag {
    #[serde(default)]
    pub liked: bool,
}

/// `GET /dislikes/status/:urn` の応答。
#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct DislikedFlag {
    #[serde(default)]
    pub disliked: bool,
}

impl Track {
    pub fn display_title(&self) -> &str {
        if self.title.is_empty() {
            "(untitled)"
        } else {
            &self.title
        }
    }

    pub fn artist_name(&self) -> &str {
        match self.user.as_ref() {
            Some(u) if !u.username.is_empty() => &u.username,
            _ => "Unknown artist",
        }
    }

    /// SoundCloud アートワークのサイズ指定 (`-large` → `-t300x300` 等)。
    /// 対応: `desktop/src/lib/formatters.ts` の `art()`。
    pub fn artwork(&self, size: &str) -> Option<String> {
        self.artwork_url
            .as_deref()
            .map(|u| u.replace("-large", &format!("-{size}")))
    }

    pub fn duration_secs(&self) -> f64 {
        self.duration as f64 / 1000.0
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Paged<T> {
    #[serde(default)]
    pub collection: Vec<T>,
    pub next_href: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct DiscoverItem {
    pub kind: Option<String>,
    pub urn: Option<String>,
    #[serde(default)]
    pub title: String,
    pub artwork_url: Option<String>,
    pub description: Option<String>,
    pub short_description: Option<String>,
    pub playlist_type: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct DiscoverSelection {
    #[serde(default)]
    pub urn: String,
    #[serde(default)]
    pub title: String,
    pub items: Option<Paged<DiscoverItem>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct DiscoverMixed {
    #[serde(default)]
    pub collection: Vec<DiscoverSelection>,
}

/// 配列・`{collection}`・`{track}` ラップのいずれも受け付ける寛容パーサ。
pub fn tracks_from_value(v: &Value) -> Vec<Track> {
    let items: &[Value] = if let Some(arr) = v.as_array() {
        arr
    } else if let Some(arr) = v.get("collection").and_then(|c| c.as_array()) {
        arr
    } else {
        return Vec::new();
    };
    items
        .iter()
        .map(|it| it.get("track").unwrap_or(it))
        .filter_map(|it| serde_json::from_value(it.clone()).ok())
        .collect()
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Playlist {
    #[serde(default)]
    pub urn: String,
    #[serde(default)]
    pub title: String,
    pub artwork_url: Option<String>,
    pub permalink_url: Option<String>,
    pub description: Option<String>,
    pub tracks: Option<Value>,
    pub user: Option<ScUser>,
    pub likes_count: Option<i64>,
    pub sharing: Option<String>,
}

impl Playlist {
    pub fn track_list(&self) -> Vec<Track> {
        self.tracks
            .as_ref()
            .map(tracks_from_value)
            .unwrap_or_default()
    }

    pub fn artwork(&self, size: &str) -> Option<String> {
        self.artwork_url
            .as_deref()
            .map(|u| u.replace("-large", &format!("-{size}")))
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Album {
    #[serde(default)]
    pub urn: String,
    #[serde(default)]
    pub title: String,
    pub artwork_url: Option<String>,
    pub description: Option<String>,
    pub tracks: Option<Value>,
    pub user: Option<ScUser>,
}

impl Album {
    pub fn track_list(&self) -> Vec<Track> {
        self.tracks
            .as_ref()
            .map(tracks_from_value)
            .unwrap_or_default()
    }

    pub fn artwork(&self, size: &str) -> Option<String> {
        self.artwork_url
            .as_deref()
            .map(|u| u.replace("-large", &format!("-{size}")))
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Comment {
    pub id: Option<i64>,
    #[serde(default)]
    pub body: String,
    pub text: Option<String>,
    pub created_at: Option<String>,
    pub timestamp: Option<i64>,
    pub user: Option<ScUser>,
}

impl Comment {
    pub fn text(&self) -> &str {
        if !self.body.is_empty() {
            &self.body
        } else {
            self.text.as_deref().unwrap_or("")
        }
    }

    pub fn author(&self) -> &str {
        match self.user.as_ref() {
            Some(u) if !u.username.is_empty() => &u.username,
            _ => "Unknown",
        }
    }
}
