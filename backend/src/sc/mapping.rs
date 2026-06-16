//! Transport-independent apiv2 → apiv1-shape mapping.
//!
//! The relay (Lua) and the apiv2-proxy channel both return raw apiv2 JSON; the backend
//! persists in the apiv1 shape its repositories already understand. These helpers do
//! the few mappings apiv2 needs — alias `likes_count`→`favoritings_count`, synthesize
//! the missing `urn`, unwrap like-feed wrappers, and reassemble a playlist's full track
//! list from id-ordered batch hydration. Used by `sc::read` (channel B) and mirrored by
//! the Lua scripts (channel A) so both channels yield identical JSON.

use std::collections::HashMap;

use serde_json::{Map, Value};

/// A public per-user collection readable anonymously via apiv2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicCollection {
    TrackLikes,
    PlaylistLikes,
    Playlists,
    Followings,
    OwnedTracks,
}

impl PublicCollection {
    /// The `kind` token understood by `sc_methods/user_collection.lua`.
    pub fn lua_kind(self) -> &'static str {
        match self {
            Self::TrackLikes => "track_likes",
            Self::PlaylistLikes => "playlist_likes",
            Self::Playlists => "playlists",
            Self::Followings => "followings",
            Self::OwnedTracks => "tracks",
        }
    }

    /// apiv2 path segment after `/users/{id}`.
    pub fn path_segment(self) -> &'static str {
        match self {
            Self::TrackLikes => "track_likes",
            Self::PlaylistLikes => "playlist_likes",
            Self::Playlists => "playlists",
            Self::Followings => "followings",
            Self::OwnedTracks => "tracks",
        }
    }

    /// Field that wraps the entity in apiv2 like-feeds (`{created_at, kind, track}`),
    /// or None when the collection already returns bare entities.
    pub fn unwrap_field(self) -> Option<&'static str> {
        match self {
            Self::TrackLikes => Some("track"),
            Self::PlaylistLikes => Some("playlist"),
            Self::Playlists | Self::Followings | Self::OwnedTracks => None,
        }
    }
}

/// A search type, matching `sc_methods/search.lua` and apiv2's `/search/{type}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchType {
    Tracks,
    Users,
    PlaylistsWithoutAlbums,
}

impl SearchType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tracks => "tracks",
            Self::Users => "users",
            Self::PlaylistsWithoutAlbums => "playlists_without_albums",
        }
    }
}

/// Unwrap a raw apiv2 collection into bare, apiv1-normalized entities. Drops malformed
/// items (no inner object / no `id`) so a junk row never reaches persistence.
pub fn unwrap_collection_items(items: &[Value], coll: PublicCollection) -> Vec<Value> {
    let unwrap = coll.unwrap_field();
    items
        .iter()
        .filter_map(|item| {
            let mut entity = match unwrap {
                Some(field) => item.get(field).cloned()?,
                None => item.clone(),
            };
            entity.get("id")?;
            normalize_v2_to_v1(&mut entity);
            Some(entity)
        })
        .collect()
}

/// Collect a playlist's track ids in order plus any already-full embedded objects
/// (apiv2 embeds ~5 full tracks; the rest are `{id, kind}` stubs). Ids are stringified
/// to match `tracks?ids` hydration keys.
pub fn collect_playlist_track_ids(playlist: &Value) -> (Vec<String>, HashMap<String, Value>) {
    let mut ids = Vec::new();
    let mut embedded = HashMap::new();
    if let Some(arr) = playlist.get("tracks").and_then(Value::as_array) {
        for t in arr {
            let Some(key) = t.get("id").and_then(id_to_string) else {
                continue;
            };
            ids.push(key.clone());
            // A stub has only id/kind; a full object carries a title.
            if t.get("title").is_some() {
                embedded.insert(key, t.clone());
            }
        }
    }
    (ids, embedded)
}

/// Rebuild a playlist's `tracks` as a full, ordered list from embedded-full objects and
/// id→track hydration results. Ids missing from both (deleted/private) are dropped, and
/// every kept track is apiv1-normalized.
pub fn reassemble_playlist_tracks(
    ordered_ids: &[String],
    embedded_full: &HashMap<String, Value>,
    hydrated: &HashMap<String, Value>,
) -> Vec<Value> {
    ordered_ids
        .iter()
        .filter_map(|id| embedded_full.get(id).or_else(|| hydrated.get(id)).cloned())
        .map(|mut t| {
            normalize_v2_to_v1(&mut t);
            t
        })
        .collect()
}

/// Index a `tracks?ids` hydration response by stringified id.
pub fn index_tracks_by_id(tracks: &[Value]) -> HashMap<String, Value> {
    tracks
        .iter()
        .filter_map(|t| Some((t.get("id").and_then(id_to_string)?, t.clone())))
        .collect()
}

fn id_to_string(v: &Value) -> Option<String> {
    match v {
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// Recursively bring an apiv2 object tree into the apiv1 shape the repositories expect:
/// alias `likes_count`→`favoritings_count` and synthesize a missing/`null` `urn`.
pub fn normalize_v2_to_v1(value: &mut Value) {
    match value {
        Value::Object(obj) => {
            normalize_object(obj);
            for (_, v) in obj.iter_mut() {
                normalize_v2_to_v1(v);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                normalize_v2_to_v1(v);
            }
        }
        _ => {}
    }
}

fn normalize_object(obj: &mut Map<String, Value>) {
    if !obj.contains_key("favoritings_count") {
        if let Some(v) = obj.get("likes_count").cloned() {
            obj.insert("favoritings_count".to_string(), v);
        }
    }
    if !matches!(obj.get("urn"), Some(Value::String(_))) {
        if let Some(urn) = synth_urn(obj) {
            obj.insert("urn".to_string(), Value::String(urn));
        }
    }
}

fn synth_urn(obj: &Map<String, Value>) -> Option<String> {
    let kind = obj.get("kind").and_then(|v| v.as_str())?;
    let segment = match kind {
        "track" => "tracks",
        "playlist" => "playlists",
        "user" => "users",
        "system-playlist" => "system-playlists",
        _ => return None,
    };
    let id = obj.get("id").and_then(id_to_string)?;
    Some(format!("soundcloud:{segment}:{id}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Shapes captured live from apiv2 during design (see plan).
    const TRACK_LIKES_PAGE: &str = r#"{
        "collection": [
            {"created_at": "2019-10-06T06:37:19Z", "kind": "like",
             "track": {"id": 588195402, "kind": "track", "title": "A", "likes_count": 7}},
            {"created_at": "2019-10-05T06:37:19Z", "kind": "like",
             "track": {"id": 100, "kind": "track", "title": "B", "likes_count": 3,
                       "urn": "soundcloud:tracks:100"}}
        ],
        "next_href": "https://api-v2.soundcloud.com/users/183/track_likes?offset=x&limit=2"
    }"#;

    const PLAYLIST_LIKES_PAGE: &str = r#"{
        "collection": [
            {"created_at": "2020-01-01T00:00:00Z", "kind": "playlist-like",
             "playlist": {"id": 7, "kind": "playlist", "title": "Mix", "likes_count": 9}}
        ]
    }"#;

    #[test]
    fn unwrap_track_likes_yields_bare_normalized_tracks() {
        let page: Value = serde_json::from_str(TRACK_LIKES_PAGE).unwrap();
        let items = page["collection"].as_array().unwrap();
        let out = unwrap_collection_items(items, PublicCollection::TrackLikes);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["id"], 588195402);
        assert_eq!(out[0]["title"], "A");
        // likes_count aliased + urn synthesized by normalize.
        assert_eq!(out[0]["favoritings_count"], 7);
        assert_eq!(out[0]["urn"], "soundcloud:tracks:588195402");
        // pre-existing urn preserved.
        assert_eq!(out[1]["urn"], "soundcloud:tracks:100");
    }

    #[test]
    fn unwrap_playlist_likes_unwraps_playlist_field() {
        let page: Value = serde_json::from_str(PLAYLIST_LIKES_PAGE).unwrap();
        let items = page["collection"].as_array().unwrap();
        let out = unwrap_collection_items(items, PublicCollection::PlaylistLikes);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["id"], 7);
        assert_eq!(out[0]["urn"], "soundcloud:playlists:7");
        assert_eq!(out[0]["favoritings_count"], 9);
    }

    #[test]
    fn bare_collection_is_not_unwrapped() {
        let items = vec![json!({"id": 1, "kind": "user", "username": "x"})];
        let out = unwrap_collection_items(&items, PublicCollection::Followings);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["urn"], "soundcloud:users:1");
    }

    #[test]
    fn malformed_items_dropped() {
        let items = vec![
            json!({"created_at": "t", "kind": "like"}), // no track
            json!({"kind": "like", "track": {"kind": "track"}}), // no id
        ];
        let out = unwrap_collection_items(&items, PublicCollection::TrackLikes);
        assert!(out.is_empty());
    }

    #[test]
    fn playlist_hydration_preserves_order_and_drops_missing() {
        // Playlist embeds id 290 full + stubs 293, 999 (999 will be missing on hydrate).
        let playlist = json!({
            "id": 18, "kind": "playlist",
            "tracks": [
                {"id": 290, "kind": "track", "title": "City Ports", "likes_count": 1},
                {"id": 293, "kind": "track"},
                {"id": 999, "kind": "track"}
            ]
        });
        let (ids, embedded) = collect_playlist_track_ids(&playlist);
        assert_eq!(ids, vec!["290", "293", "999"]);
        assert_eq!(embedded.len(), 1);
        assert!(embedded.contains_key("290"));

        // Hydration returns 293 but not 999 (deleted).
        let hydrated =
            index_tracks_by_id(&[json!({"id": 293, "kind": "track", "title": "Flickermood"})]);
        let tracks = reassemble_playlist_tracks(&ids, &embedded, &hydrated);
        let got: Vec<i64> = tracks.iter().map(|t| t["id"].as_i64().unwrap()).collect();
        assert_eq!(got, vec![290, 293]); // order kept, 999 dropped
        assert_eq!(tracks[0]["title"], "City Ports");
        assert_eq!(tracks[0]["favoritings_count"], 1); // normalized
    }

    #[test]
    fn collect_handles_string_ids() {
        let playlist = json!({"id": 1, "tracks": [{"id": "abc", "title": "t"}]});
        let (ids, embedded) = collect_playlist_track_ids(&playlist);
        assert_eq!(ids, vec!["abc"]);
        assert!(embedded.contains_key("abc"));
    }
}
