use base64::Engine;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistCursor {
    /// Primary numeric ordering value (trending/listeners/tracks/star).
    pub p: f64,
    /// Secondary numeric ordering value (used by `star` to break ties by trending).
    #[serde(default)]
    pub p2: f64,
    /// Tiebreaker — normalized_name.
    pub n: String,
    /// Final stable tiebreaker.
    pub id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumCursor {
    /// Primary numeric (popularity, track_count, year+month-ordinal, …).
    pub p: f64,
    /// Secondary numeric (release_date as days since epoch, used by `recent`).
    #[serde(default)]
    pub p2: f64,
    /// Tiebreaker — normalized_title.
    pub n: String,
    pub id: Uuid,
}

pub fn encode<T: Serialize>(c: &T) -> String {
    let json = serde_json::to_vec(c).expect("cursor serialization");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json)
}

pub fn decode<T: for<'de> Deserialize<'de>>(s: &str) -> AppResult<T> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s.as_bytes())
        .map_err(|_| AppError::bad_request("invalid cursor"))?;
    serde_json::from_slice(&bytes).map_err(|_| AppError::bad_request("invalid cursor"))
}
