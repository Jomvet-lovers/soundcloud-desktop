use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct LikedTracksQuery {
    #[serde(default)]
    pub access: Option<String>,
}
