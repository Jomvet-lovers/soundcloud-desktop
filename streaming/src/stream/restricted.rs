use reqwest::Client;
use std::collections::HashMap;

use super::proxy::{fetch_get_json, fetch_get_text};

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, serde::Deserialize)]
pub struct TranscodingFormat {
    pub protocol: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct Transcoding {
    pub url: String,
    pub preset: Option<String>,
    pub snipped: Option<bool>,
    pub quality: Option<String>,
    pub format: Option<TranscodingFormat>,
}

pub(crate) struct RestrictedSource {
    pub manifest: String,
    pub token: String,
    pub content_type: &'static str,
}

#[derive(serde::Deserialize)]
struct ResolveResp {
    url: String,
    #[serde(rename = "licenseAuthToken")]
    auth_token: Option<String>,
}

pub fn build_transcoding_target(
    transcoding_url: &str,
    client_id: &str,
    track_authorization: Option<&str>,
) -> String {
    let sep = if transcoding_url.contains('?') {
        "&"
    } else {
        "?"
    };
    let mut target = format!("{transcoding_url}{sep}client_id={client_id}");
    if let Some(auth) = track_authorization.filter(|a| !a.is_empty()) {
        target.push_str("&track_authorization=");
        target.push_str(auth);
    }
    target
}

fn content_type_from_mime(mime: Option<&str>) -> &'static str {
    match mime.map(|m| m.split(';').next().unwrap_or("").trim()) {
        Some("audio/mpeg") => "audio/mpeg",
        Some("audio/ogg") => "audio/ogg",
        _ => "audio/mp4",
    }
}

/// Pick the `ctr` transcoding, resolve it, return manifest + token.
/// `headers` carries the caller's identity (empty = anon, OAuth = cookies);
/// the same headers are reused for both the resolve and the manifest fetch.
pub(crate) async fn resolve(
    client: &Client,
    proxy_url: &str,
    transcodings: &[Transcoding],
    client_id: &str,
    track_auth: Option<&str>,
    headers: HashMap<String, String>,
) -> Result<Option<RestrictedSource>, BoxErr> {
    let tc = match transcodings.iter().find(|t| {
        t.format.as_ref().and_then(|f| f.protocol.as_deref()) == Some("ctr-encrypted-hls")
    }) {
        Some(t) => t,
        None => return Ok(None),
    };
    let content_type =
        content_type_from_mime(tc.format.as_ref().and_then(|f| f.mime_type.as_deref()));

    let target = build_transcoding_target(&tc.url, client_id, track_auth);
    let r: ResolveResp = fetch_get_json(client, proxy_url, &target, headers.clone(), false).await?;
    let token = match r.auth_token {
        Some(t) => t,
        None => return Ok(None),
    };
    let (manifest, _) = fetch_get_text(client, proxy_url, &r.url, headers, false).await?;

    Ok(Some(RestrictedSource {
        manifest,
        token,
        content_type,
    }))
}
