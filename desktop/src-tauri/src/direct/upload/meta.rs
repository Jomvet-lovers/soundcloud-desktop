//! Track creation payloads and upload-response parsing.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct UploadMeta {
    pub title: String,
    pub artist: Option<String>,
    pub genre: Option<String>,
    #[serde(alias = "tagList")]
    pub tag_list: Option<String>,
    pub description: Option<String>,
    pub sharing: Option<String>,
    #[serde(alias = "embeddableBy")]
    pub embeddable_by: Option<String>,
    pub permalink: Option<String>,
    #[serde(alias = "releaseDate")]
    pub release_date: Option<String>,
    pub downloadable: Option<bool>,
    pub commentable: Option<bool>,
}

#[derive(Serialize)]
pub struct UploadedTrack {
    pub id: u64,
    pub urn: String,
    pub permalink_url: String,
    pub title: String,
    pub state: String,
    /// Present when the track was created but the artwork PUT failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artwork_error: Option<String>,
}

/// Track attributes for the JSON create call. `title` is required; optional
/// text fields are dropped when absent or empty. `downloadable` / `commentable`
/// default to SoundCloud's `true` when the caller omits them.
pub(super) fn track_meta_json(meta: &UploadMeta) -> Result<Value, String> {
    if meta.title.trim().is_empty() {
        return Err("Title is required".into());
    }
    if let Some(sharing) = meta.sharing.as_deref().filter(|s| !s.is_empty()) {
        if sharing != "public" && sharing != "private" {
            return Err(format!("Invalid sharing setting: {sharing}"));
        }
    }
    let mut obj = serde_json::Map::new();
    obj.insert("title".into(), json!(meta.title));
    let texts: [(&str, Option<&String>); 7] = [
        ("genre", meta.genre.as_ref()),
        ("tag_list", meta.tag_list.as_ref()),
        ("description", meta.description.as_ref()),
        ("sharing", meta.sharing.as_ref()),
        // The web app sends `embeddable` (the model attribute is
        // `embeddable_by`, but the save payload renames it).
        ("embeddable", meta.embeddable_by.as_ref()),
        ("permalink", meta.permalink.as_ref()),
        ("release_date", meta.release_date.as_ref()),
    ];
    for (key, value) in texts {
        if let Some(value) = value.filter(|v| !v.is_empty()) {
            obj.insert(key.into(), json!(value));
        }
    }
    if let Some(artist) = meta.artist.as_ref().filter(|v| !v.trim().is_empty()) {
        obj.insert("publisher_metadata".into(), json!({ "artist": artist }));
    }
    obj.insert(
        "downloadable".into(),
        json!(meta.downloadable.unwrap_or(true)),
    );
    obj.insert("commentable".into(), json!(meta.commentable.unwrap_or(true)));
    Ok(Value::Object(obj))
}

/// Map the upload response into the wire shape the frontend consumes. The
/// writer channel truncates long payloads, so a regex recovers the id from a
/// cut-off body (same fallback as `routes/normalize.rs`).
pub(super) fn parse_uploaded_track(payload: &str) -> Result<UploadedTrack, String> {
    if let Ok(v) = serde_json::from_str::<Value>(payload) {
        if let Some(id) = v.get("id").and_then(Value::as_u64) {
            let text = |key: &str| {
                v.get(key)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            let state = text("state");
            return Ok(UploadedTrack {
                id,
                urn: v
                    .get("urn")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("soundcloud:tracks:{id}")),
                permalink_url: text("permalink_url"),
                title: text("title"),
                state: if state.is_empty() {
                    "processing".into()
                } else {
                    state
                },
                artwork_error: v
                    .get("artwork_error")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            });
        }
    }
    match id_from_payload(payload) {
        Some(id) => Ok(UploadedTrack {
            id,
            urn: format!("soundcloud:tracks:{id}"),
            permalink_url: String::new(),
            title: String::new(),
            state: "processing".into(),
            artwork_error: None,
        }),
        None => Err("Track ID was not found in the response".into()),
    }
}

fn id_from_payload(payload: &str) -> Option<u64> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r#""id"\s*:\s*(\d+)"#).expect("static pattern"));
    re.captures(payload)?.get(1)?.as_str().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(value: Value) -> UploadMeta {
        serde_json::from_value(value).expect("meta should deserialize")
    }

    #[test]
    fn track_meta_requires_title() {
        assert!(track_meta_json(&meta(json!({"title": ""}))).is_err());
        assert!(track_meta_json(&meta(json!({"title": "   "}))).is_err());
        let value = track_meta_json(&meta(json!({"title": "Track"}))).unwrap();
        assert_eq!(value["title"], "Track");
    }

    #[test]
    fn track_meta_skips_absent_and_empty_optionals() {
        let value = track_meta_json(&meta(json!({
            "title": "T",
            "artist": "",
            "genre": null,
            "description": null
        })))
        .unwrap();
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, vec!["commentable", "downloadable", "title"]);
    }

    #[test]
    fn track_meta_includes_provided_values() {
        let value = track_meta_json(&meta(json!({
            "title": "T",
            "artist": "A",
            "genre": "Techno",
            "tag_list": "a,b",
            "description": "d",
            "sharing": "private",
            "embeddable_by": "me",
            "permalink": "my-track",
            "release_date": "2026-10-19"
        })))
        .unwrap();
        for (key, expected) in [
            ("title", "T"),
            ("genre", "Techno"),
            ("tag_list", "a,b"),
            ("description", "d"),
            ("sharing", "private"),
            ("embeddable", "me"),
            ("permalink", "my-track"),
            ("release_date", "2026-10-19"),
        ] {
            assert_eq!(value[key], expected, "missing {key}");
        }
        assert_eq!(value["publisher_metadata"]["artist"], "A");
    }

    #[test]
    fn track_meta_defaults_flags_to_true() {
        let value = track_meta_json(&meta(json!({"title": "T"}))).unwrap();
        assert_eq!(value["downloadable"], true);
        assert_eq!(value["commentable"], true);

        let off = track_meta_json(&meta(json!({
            "title": "T",
            "downloadable": false,
            "commentable": false
        })))
        .unwrap();
        assert_eq!(off["downloadable"], false);
        assert_eq!(off["commentable"], false);
    }

    #[test]
    fn track_meta_rejects_bad_sharing() {
        assert!(track_meta_json(&meta(json!({"title": "T", "sharing": "unlisted"}))).is_err());
        // The empty string is the omitted-value convention, not an error.
        assert!(track_meta_json(&meta(json!({"title": "T", "sharing": ""}))).is_ok());
    }

    #[test]
    fn track_meta_accepts_camel_case_keys() {
        let value = track_meta_json(&meta(json!({
            "title": "T",
            "tagList": "a,b",
            "embeddableBy": "me",
            "releaseDate": "2026-10-19"
        })))
        .unwrap();
        assert_eq!(value["tag_list"], "a,b");
        assert_eq!(value["embeddable"], "me");
        assert_eq!(value["release_date"], "2026-10-19");
    }

    #[test]
    fn parse_uploaded_track_reads_json() {
        let track = parse_uploaded_track(
            r#"{"id":42,"urn":"soundcloud:tracks:42","permalink_url":"https://soundcloud.com/x/y","title":"T","state":"processing"}"#,
        )
        .unwrap();
        assert_eq!(track.id, 42);
        assert_eq!(track.urn, "soundcloud:tracks:42");
        assert_eq!(track.permalink_url, "https://soundcloud.com/x/y");
        assert_eq!(track.title, "T");
        assert_eq!(track.state, "processing");
    }

    #[test]
    fn parse_uploaded_track_fills_defaults() {
        let track = parse_uploaded_track(r#"{"id":7,"title":"X"}"#).unwrap();
        assert_eq!(track.urn, "soundcloud:tracks:7");
        assert_eq!(track.permalink_url, "");
        assert_eq!(track.title, "X");
        assert_eq!(track.state, "processing");
    }

    #[test]
    fn parse_uploaded_track_regex_fallback_for_truncated_json() {
        let track = parse_uploaded_track(r#"{"id": 12345, "title":"cut off mid..."#).unwrap();
        assert_eq!(track.id, 12345);
        assert_eq!(track.urn, "soundcloud:tracks:12345");
        assert_eq!(track.title, "");
        assert_eq!(track.state, "processing");
    }

    #[test]
    fn parse_uploaded_track_without_id_is_err() {
        assert!(parse_uploaded_track(r#"{"error":"bad request"}"#).is_err());
        assert!(parse_uploaded_track("not json").is_err());
    }

    #[test]
    fn parse_uploaded_track_keeps_the_artwork_error() {
        let track = parse_uploaded_track(
            r#"{"id":9,"title":"T","artwork_error":"artwork upload 400 Invalid request"}"#,
        )
        .unwrap();
        assert_eq!(
            track.artwork_error.as_deref(),
            Some("artwork upload 400 Invalid request")
        );
        let without = parse_uploaded_track(r#"{"id":9,"title":"T"}"#).unwrap();
        assert_eq!(without.artwork_error, None);
    }
}
