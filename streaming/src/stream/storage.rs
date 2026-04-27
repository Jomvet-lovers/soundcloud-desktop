use bytes::Bytes;
use reqwest::Client;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use tracing::{info, warn};

use crate::config::Config;
use crate::db::postgres::PgPool;

const UNAVAILABLE_THRESHOLD: u32 = 3;
const UNAVAILABLE_COOLDOWN_MS: u64 = 60_000;

pub struct StorageClient {
    client: Client,
    base_url: String,
    public_url: String,
    upload_url: String,
    auth_token: String,
    pg: PgPool,
    consecutive_unavailable: Arc<AtomicU32>,
    unavailable_until: Arc<AtomicU64>,
}

impl StorageClient {
    pub fn new(client: Client, config: &Config, pg: PgPool) -> Self {
        Self {
            client,
            base_url: config.storage_url.trim_end_matches('/').to_string(),
            public_url: config.storage_public_url.trim_end_matches('/').to_string(),
            upload_url: config.storage_upload_url.trim_end_matches('/').to_string(),
            auth_token: config.storage_token.clone(),
            pg,
            consecutive_unavailable: Arc::new(AtomicU32::new(0)),
            unavailable_until: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn enabled(&self) -> bool {
        !self.base_url.is_empty() && !self.auth_token.is_empty()
    }

    pub fn track_filename(track_urn: &str) -> String {
        track_urn.replace(':', "_")
    }

    pub fn track_path(track_urn: &str, quality: &str) -> String {
        format!("{quality}/{}.ogg", Self::track_filename(track_urn))
    }

    pub fn internal_url(&self, track_urn: &str, quality: &str) -> String {
        format!("{}/{}", self.base_url, Self::track_path(track_urn, quality))
    }

    pub fn public_track_url(&self, track_urn: &str, quality: &str) -> String {
        format!(
            "{}/{}",
            self.public_url,
            Self::track_path(track_urn, quality)
        )
    }

    fn is_temporarily_unavailable(&self) -> bool {
        let until = self.unavailable_until.load(Ordering::Relaxed);
        until > 0 && now_ms() < until
    }

    pub async fn try_serve(&self, track_urn: &str, prefer_hq: bool) -> Option<String> {
        if !self.enabled() || self.is_temporarily_unavailable() {
            return None;
        }

        let cached = self
            .pg
            .find_cached_track(track_urn, prefer_hq)
            .await
            .ok()??;
        let verify_url = self.internal_url(track_urn, &cached.quality);

        match self.verify_url(&verify_url).await {
            VerifyResult::Ok => {
                let _ = self.pg.update_last_accessed(&cached.id).await;
                Some(self.public_track_url(track_urn, &cached.quality))
            }
            VerifyResult::Missing => {
                let _ = self.pg.update_cdn_track_status(&cached.id, "error").await;
                None
            }
            VerifyResult::Unavailable => None,
        }
    }

    pub fn upload_in_background(&self, track_urn: String, data: Bytes) {
        if !self.enabled() || self.is_temporarily_unavailable() {
            return;
        }

        let client = self.client.clone();
        let upload_url = self.upload_url.clone();
        let auth_token = self.auth_token.clone();
        let pg = self.pg.clone();
        let filename = Self::track_filename(&track_urn);
        let consec = self.consecutive_unavailable.clone();
        let until = self.unavailable_until.clone();

        tokio::spawn(async move {
            let hq_path = Self::track_path(&track_urn, "hq");
            let sq_path = Self::track_path(&track_urn, "sq");

            let hq_id = match pg
                .insert_cdn_track(&track_urn, "hq", &hq_path, "pending")
                .await
            {
                Ok(id) => id,
                Err(e) => {
                    warn!("[storage] insert pending hq failed: {e}");
                    return;
                }
            };
            let sq_id = match pg
                .insert_cdn_track(&track_urn, "sq", &sq_path, "pending")
                .await
            {
                Ok(id) => id,
                Err(e) => {
                    warn!("[storage] insert pending sq failed: {e}");
                    return;
                }
            };

            match upload_to_storage(&client, &upload_url, &auth_token, &filename, &data).await {
                Ok(()) => {
                    consec.store(0, Ordering::Relaxed);
                    until.store(0, Ordering::Relaxed);
                    let _ = pg.update_cdn_track_status(&hq_id, "ok").await;
                    let _ = pg.update_cdn_track_status(&sq_id, "ok").await;
                    info!(
                        "[storage] uploaded {} ({:.1} MB)",
                        filename,
                        data.len() as f64 / 1024.0 / 1024.0
                    );
                    // storage.track_uploaded NATS event теперь публикует сам storage —
                    // он узнаёт о реальном завершении S3 PUT после ретраев, а не о HTTP 200.
                }
                Err(e) => {
                    let prev = consec.fetch_add(1, Ordering::Relaxed);
                    let cur_until = until.load(Ordering::Relaxed);
                    if prev + 1 >= UNAVAILABLE_THRESHOLD && now_ms() >= cur_until {
                        until.store(now_ms() + UNAVAILABLE_COOLDOWN_MS, Ordering::Relaxed);
                        warn!("[storage] breaker opened after {} upload failures", prev + 1);
                    }
                    warn!("[storage] upload failed for {filename}: {e}");
                    let _ = pg.update_cdn_track_status(&hq_id, "error").await;
                    let _ = pg.update_cdn_track_status(&sq_id, "error").await;
                }
            }
        });
    }

    pub async fn delete_file(&self, track_urn: &str) -> Result<(), reqwest::Error> {
        let filename = Self::track_filename(track_urn);
        let url = format!("{}/files/{}", self.base_url, filename);
        self.client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    async fn verify_url(&self, url: &str) -> VerifyResult {
        if self.is_temporarily_unavailable() {
            return VerifyResult::Unavailable;
        }

        match self
            .client
            .head(url)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
        {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if (200..300).contains(&status) {
                    self.mark_available();
                    VerifyResult::Ok
                } else if status == 404 || status == 410 {
                    self.mark_available();
                    VerifyResult::Missing
                } else {
                    self.mark_unavailable();
                    VerifyResult::Unavailable
                }
            }
            Err(_) => {
                self.mark_unavailable();
                VerifyResult::Unavailable
            }
        }
    }

    fn mark_available(&self) {
        self.consecutive_unavailable.store(0, Ordering::Relaxed);
        self.unavailable_until.store(0, Ordering::Relaxed);
    }

    fn mark_unavailable(&self) {
        let prev = self.consecutive_unavailable.fetch_add(1, Ordering::Relaxed);
        if prev + 1 >= UNAVAILABLE_THRESHOLD && !self.is_temporarily_unavailable() {
            self.unavailable_until
                .store(now_ms() + UNAVAILABLE_COOLDOWN_MS, Ordering::Relaxed);
            warn!("[storage] breaker opened after {} failures", prev + 1);
        }
    }
}

enum VerifyResult {
    Ok,
    Missing,
    Unavailable,
}

async fn upload_to_storage(
    client: &Client,
    base_url: &str,
    auth_token: &str,
    filename: &str,
    data: &Bytes,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let file_part = reqwest::multipart::Part::bytes(data.to_vec())
        .file_name("audio")
        .mime_str("audio/mpeg")?;

    let form = reqwest::multipart::Form::new()
        .text("filename", filename.to_string())
        .part("file", file_part);

    client
        .post(format!("{base_url}/upload"))
        .header("Authorization", format!("Bearer {auth_token}"))
        .multipart(form)
        .timeout(std::time::Duration::from_secs(600))
        .send()
        .await?
        .error_for_status()?;

    Ok(())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}