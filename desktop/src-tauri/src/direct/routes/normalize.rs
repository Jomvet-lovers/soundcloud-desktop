use serde_json::{json, Value};

pub(crate) fn normalize_urn(mut item: Value) -> Value {
    if item.get("urn").is_none() {
        if let Some(id) = item.get("id").and_then(Value::as_u64) {
            let kind = match item.get("kind").and_then(Value::as_str) {
                Some("user") | Some("artist") => "users",
                Some("playlist") | Some("album") => "playlists",
                _ => "tracks",
            };
            if let Some(obj) = item.as_object_mut() {
                obj.insert("urn".into(), json!(format!("soundcloud:{kind}:{id}")));
            }
        }
    }
    mirror_uploader_avatar(&mut item);
    item
}

/// Tracks without jacket art show the uploader's avatar (official web parity:
/// soundcloud.com falls back to the avatar when `artwork_url` is null).
/// Track-only: playlists keep their own cover logic on the frontend
/// (`playlist-cover.ts`), users already carry their own `avatar_url`.
pub(crate) fn mirror_uploader_avatar(item: &mut Value) {
    if item.get("kind").and_then(Value::as_str) != Some("track") {
        return;
    }
    let empty = item.get("artwork_url").map(Value::is_null).unwrap_or(true);
    if !empty {
        return;
    }
    let avatar = item
        .get("user")
        .and_then(|u| u.get("avatar_url"))
        .filter(|v| !v.is_null())
        .cloned();
    if let (Some(avatar), Some(obj)) = (avatar, item.as_object_mut()) {
        obj.insert("artwork_url".into(), avatar);
    }
}

pub(crate) fn urn_of(v: &Value) -> Option<String> {
    v.get("urn").and_then(Value::as_str).map(str::to_string)
}

/// SC comments carry the URN under `self.urn`; mirror it to a top-level `urn`
/// so the frontend can key on either.
pub(crate) fn normalize_comment(mut item: Value) -> Value {
    if item.get("urn").is_none() {
        let urn = item
            .get("self")
            .and_then(|s| s.get("urn"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                item.get("id")
                    .and_then(Value::as_u64)
                    .map(|id| format!("soundcloud:comments:{id}"))
            });
        if let (Some(urn), Some(obj)) = (urn, item.as_object_mut()) {
            obj.insert("urn".into(), json!(urn));
        }
    }
    item
}

/// A synced local comment must not be shown twice once SoundCloud returns it.
pub(crate) fn local_comment_synced_elsewhere(local: &Value, sc_comments: &[Value]) -> bool {
    if local.get("sync").and_then(Value::as_str) == Some("pending") {
        return false;
    }
    if let Some(id) = local.get("id").and_then(Value::as_u64) {
        if sc_comments
            .iter()
            .any(|c| c.get("id").and_then(Value::as_u64) == Some(id))
        {
            return true;
        }
    }
    let body = local.get("body").and_then(Value::as_str).unwrap_or_default();
    if body.is_empty() {
        return false;
    }
    let ts = local.get("timestamp").and_then(Value::as_u64);
    sc_comments.iter().any(|c| {
        c.get("body").and_then(Value::as_str) == Some(body)
            && c.get("timestamp").and_then(Value::as_u64) == ts
    })
}

/// Legacy local comments were saved with `user: null`; give them a renderable
/// placeholder so the list never crashes.
pub(crate) fn local_comment_user_placeholder() -> Value {
    json!({
        "id": 0,
        "urn": "local:users:me",
        "username": "You",
        "avatar_url": Value::Null,
        "permalink_url": Value::Null,
    })
}

/// Pull the SoundCloud comment id out of a writer response. The writer channel
/// truncates payloads, so fall back to a regex when the JSON is cut off.
pub(crate) fn parse_sc_comment_id(payload: &str) -> Option<u64> {
    if let Ok(v) = serde_json::from_str::<Value>(payload) {
        if let Some(id) = v.get("id").and_then(Value::as_u64) {
            return Some(id);
        }
    }
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r#""id"\s*:\s*(\d+)"#).unwrap());
    re.captures(payload)?.get(1)?.as_str().parse().ok()
}

/// Normalizes an item (and nested selection items) for the frontend:
/// - ensures a `urn` (playlists/users without one get it from `id`/`kind`),
/// - mirrors `calculated_artwork_url` / `avatar_url` into `artwork_url`,
/// - mirrors `artwork_url` into `cover_url`.
pub(crate) fn normalize_deep(mut item: Value) -> Value {
    item = normalize_urn(item);
    if item.get("artwork_url").map(Value::is_null).unwrap_or(true) {
        let fallback = item
            .get("calculated_artwork_url")
            .filter(|v| !v.is_null())
            .cloned()
            .or_else(|| item.get("avatar_url").filter(|v| !v.is_null()).cloned());
        if let Some(art) = fallback {
            if let Some(obj) = item.as_object_mut() {
                obj.insert("artwork_url".into(), art);
            }
        }
    }
    if item.get("cover_url").is_none() {
        if let Some(art) = item.get("artwork_url").cloned() {
            if let Some(obj) = item.as_object_mut() {
                obj.insert("cover_url".into(), art);
            }
        }
    }
    if let Some(collection) = item
        .get_mut("items")
        .and_then(|i| i.get_mut("collection"))
        .and_then(Value::as_array_mut)
    {
        for entry in collection.iter_mut() {
            let taken = entry.take();
            *entry = normalize_deep(taken);
        }
    }
    item
}

/// Local ordering for search results (SoundCloud ignores `sort`).
pub(crate) fn sort_tracks(items: &mut [Value], sort: &str) {
    let num = |v: &Value, keys: &[&str]| -> i64 {
        keys.iter()
            .find_map(|k| v.get(*k).and_then(Value::as_i64))
            .unwrap_or(0)
    };
    match sort {
        "plays" => items.sort_by(|a, b| {
            num(b, &["playback_count"]).cmp(&num(a, &["playback_count"]))
        }),
        "likes" => items.sort_by(|a, b| {
            num(b, &["favoritings_count", "likes_count"])
                .cmp(&num(a, &["favoritings_count", "likes_count"]))
        }),
        "newest" => items.sort_by(|a, b| {
            let ts = |v: &Value| {
                v.get("created_at")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            ts(b).cmp(&ts(a))
        }),
        _ => {}
    }
}

/// Raw SoundCloud playlist -> the `AlbumDetail` shape the frontend renders.
pub(crate) fn album_detail(v: &Value) -> Value {
    let id = v
        .get("id")
        .map(|x| x.to_string().trim_matches('"').to_string())
        .unwrap_or_default();
    let is_album = v.get("is_album").and_then(Value::as_bool).unwrap_or(false);
    let set_type = v.get("set_type").and_then(Value::as_str).unwrap_or("");
    let ty = if !set_type.is_empty() {
        set_type
    } else if is_album {
        "album"
    } else {
        "playlist"
    };
    let release_year = v
        .get("release_date")
        .and_then(Value::as_str)
        .and_then(|d| d.get(0..4))
        .and_then(|y| y.parse::<u64>().ok());
    let primary = v.get("user").and_then(|u| {
        let uid = u
            .get("id")
            .map(|x| x.to_string().trim_matches('"').to_string())?;
        Some(json!({
            "id": uid,
            "name": u.get("username").cloned().unwrap_or(Value::Null),
            "role": "primary",
            "avatar_url": u.get("avatar_url").cloned().unwrap_or(Value::Null),
        }))
    });
    let artists = match &primary {
        Some(p) => json!([p.clone()]),
        None => json!([]),
    };
    let tracks: Vec<Value> = v
        .get("tracks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(normalize_urn)
        .collect();
    json!({
        "id": id,
        "title": v.get("title").cloned().unwrap_or(Value::Null),
        "type": ty,
        "release_year": release_year,
        "cover_url": v.get("artwork_url").cloned().unwrap_or(Value::Null),
        "confidence": 1.0,
        "primary_artist": primary,
        "artists": artists,
        "tracks": tracks,
    })
}

/// Raw SoundCloud playlist -> the `ArtistAlbum` list item shape.
pub(crate) fn artist_album(p: &Value) -> Value {
    let id = p
        .get("id")
        .map(|x| x.to_string().trim_matches('"').to_string())
        .unwrap_or_default();
    let is_album = p.get("is_album").and_then(Value::as_bool).unwrap_or(false);
    let set_type = p.get("set_type").and_then(Value::as_str).unwrap_or("");
    let ty = if !set_type.is_empty() {
        set_type
    } else if is_album {
        "album"
    } else {
        "playlist"
    };
    let release_year = p
        .get("release_date")
        .and_then(Value::as_str)
        .and_then(|d| d.get(0..4))
        .and_then(|y| y.parse::<u64>().ok());
    json!({
        "id": id,
        "title": p.get("title").cloned().unwrap_or(Value::Null),
        "type": ty,
        "release_year": release_year,
        "cover_url": p.get("artwork_url").cloned().unwrap_or(Value::Null),
        "role": "primary",
    })
}

/// Raw SoundCloud user -> the `ArtistDetail` shape the frontend renders.
pub(crate) fn artist_detail(v: &Value, socials: Vec<Value>) -> Value {
    let id = v
        .get("id")
        .map(|x| x.to_string().trim_matches('"').to_string())
        .unwrap_or_default();
    let track_count = v.get("track_count").and_then(Value::as_u64).unwrap_or(0);
    let playlist_count = v
        .get("playlist_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    json!({
        "id": id,
        "name": v.get("username").cloned().unwrap_or(Value::Null),
        "country": v.get("country_code").cloned().unwrap_or(Value::Null),
        "bio": v.get("description").cloned().unwrap_or(Value::Null),
        "avatar_url": v.get("avatar_url").cloned().unwrap_or(Value::Null),
        "confidence": 1.0,
        "socials": socials,
        "sc_accounts": [],
        "track_count": track_count,
        "track_count_primary": track_count,
        "track_count_featured": 0,
        "album_count": playlist_count,
        "popular_tracks": [],
        "related_artists": [],
    })
}

#[cfg(test)]
mod tests {
    use super::normalize_urn;
    use serde_json::json;

    fn track(artwork: serde_json::Value) -> serde_json::Value {
        json!({
            "kind": "track",
            "id": 1,
            "title": "t",
            "artwork_url": artwork,
            "user": {
                "username": "u",
                "avatar_url": "https://example.com/avatar-large.jpg",
            },
        })
    }

    /// Jacket-less tracks show the uploader avatar (official web parity).
    #[test]
    fn track_without_artwork_falls_back_to_uploader_avatar() {
        let out = normalize_urn(track(serde_json::Value::Null));
        assert_eq!(
            out.get("artwork_url").and_then(|v| v.as_str()),
            Some("https://example.com/avatar-large.jpg"),
        );
    }

    /// Tracks that have their own jacket keep it.
    #[test]
    fn track_with_artwork_is_untouched() {
        let out = normalize_urn(track(json!("https://example.com/cover-large.jpg")));
        assert_eq!(
            out.get("artwork_url").and_then(|v| v.as_str()),
            Some("https://example.com/cover-large.jpg"),
        );
    }

    /// Playlists keep their own cover logic (frontend `playlist-cover.ts`):
    /// no uploader-avatar injection here.
    #[test]
    fn playlist_without_artwork_is_untouched() {
        let out = normalize_urn(json!({
            "kind": "playlist",
            "id": 2,
            "title": "p",
            "artwork_url": null,
            "user": { "avatar_url": "https://example.com/avatar-large.jpg" },
        }));
        assert!(out.get("artwork_url").map(|v| v.is_null()).unwrap_or(false));
    }
}
