use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::StreamExt;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use reqwest::{Client, StatusCode as HttpStatus};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::info;

use super::{BackendError, ByteStream, ObjectInfo};
use crate::config::{GdriveAuth, GdriveConfig};

const DRIVE_API: &str = "https://www.googleapis.com/drive/v3";
const DRIVE_UPLOAD_API: &str = "https://www.googleapis.com/upload/drive/v3";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const TOKEN_REFRESH_LEEWAY: Duration = Duration::from_secs(60);
const SCOPE: &str = "https://www.googleapis.com/auth/drive";

pub struct GdriveBackend {
    http: Client,
    cfg: GdriveConfig,
    auth: AuthState,
    token: Mutex<Option<CachedToken>>,
    public_files: Mutex<HashSet<String>>,
}

enum AuthState {
    ServiceAccount(ServiceAccount),
    UserOAuth {
        client_id: String,
        client_secret: String,
        refresh_token: String,
    },
}

#[derive(Deserialize)]
struct ServiceAccount {
    client_email: String,
    private_key: String,
    #[serde(default)]
    token_uri: Option<String>,
}

#[derive(Clone)]
struct CachedToken {
    value: String,
    expires_at: SystemTime,
}

#[derive(Serialize)]
struct JwtClaims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    exp: u64,
    iat: u64,
}

#[derive(Deserialize)]
struct TokenResp {
    access_token: String,
    expires_in: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DriveFile {
    id: String,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    mime_type: Option<String>,
}

#[derive(Deserialize)]
struct ListResp {
    #[serde(default)]
    files: Vec<DriveFile>,
}

impl GdriveBackend {
    pub async fn new(cfg: &GdriveConfig) -> Result<Self, BackendError> {
        let auth = match &cfg.auth {
            GdriveAuth::ServiceAccount(json) => {
                let sa: ServiceAccount = serde_json::from_str(json).map_err(|e| {
                    BackendError::Other(format!("parse service account JSON: {e}"))
                })?;
                AuthState::ServiceAccount(sa)
            }
            GdriveAuth::UserOAuth {
                client_id,
                client_secret,
                refresh_token,
            } => AuthState::UserOAuth {
                client_id: client_id.clone(),
                client_secret: client_secret.clone(),
                refresh_token: refresh_token.clone(),
            },
        };

        let http = Client::builder()
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .map_err(|e| BackendError::Other(format!("http client: {e}")))?;

        let backend = Self {
            http,
            cfg: cfg.clone(),
            auth,
            token: Mutex::new(None),
            public_files: Mutex::new(HashSet::new()),
        };

        backend
            .access_token()
            .await
            .map_err(|e| BackendError::Other(format!("auth check: {e}")))?;
        let mode = match &backend.auth {
            AuthState::ServiceAccount(_) => "service_account",
            AuthState::UserOAuth { .. } => "user_oauth",
        };
        info!(
            "[gdrive] auth ok mode={mode} root_folder_id={}",
            backend.cfg.root_folder_id
        );

        Ok(backend)
    }

    pub async fn put_file(&self, key: &str, src: &Path) -> Result<(), BackendError> {
        let mime = super::content_type_for(key);
        let existing = self.find_in_folder(&self.cfg.root_folder_id, key).await?;
        let existing_id = existing.as_ref().map(|f| f.id.as_str());
        self.upload_resumable(&self.cfg.root_folder_id, key, mime, src, existing_id)
            .await?;
        let _ = tokio::fs::remove_file(src).await;
        Ok(())
    }

    pub async fn delete_file(&self, key: &str) -> Result<bool, BackendError> {
        let Some(file) = self.find_in_folder(&self.cfg.root_folder_id, key).await? else {
            return Ok(false);
        };
        let token = self.access_token().await?;
        let resp = self
            .http
            .delete(format!(
                "{DRIVE_API}/files/{}?supportsAllDrives=true",
                file.id
            ))
            .bearer_auth(&token)
            .send()
            .await
            .map_err(http_err)?;
        if resp.status() == HttpStatus::NOT_FOUND {
            return Ok(false);
        }
        if !resp.status().is_success() {
            let s = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(BackendError::Other(format!("delete {key}: {s}: {body}")));
        }
        self.public_files.lock().await.remove(&file.id);
        Ok(true)
    }

    pub async fn head(&self, key: &str) -> Result<Option<ObjectInfo>, BackendError> {
        let Some(file) = self.find_in_folder(&self.cfg.root_folder_id, key).await? else {
            return Ok(None);
        };
        Ok(Some(ObjectInfo {
            size: parse_size(file.size.as_deref()),
            content_type: file
                .mime_type
                .or_else(|| Some(super::content_type_for(key).to_string())),
        }))
    }

    pub async fn stream(
        &self,
        key: &str,
    ) -> Result<(ObjectInfo, ByteStream), BackendError> {
        let Some(file) = self.find_in_folder(&self.cfg.root_folder_id, key).await? else {
            return Err(BackendError::NotFound);
        };
        let token = self.access_token().await?;
        let resp = self
            .http
            .get(format!(
                "{DRIVE_API}/files/{}?alt=media&supportsAllDrives=true",
                file.id
            ))
            .bearer_auth(&token)
            .send()
            .await
            .map_err(http_err)?;
        if resp.status() == HttpStatus::NOT_FOUND {
            return Err(BackendError::NotFound);
        }
        if !resp.status().is_success() {
            let s = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(BackendError::Other(format!("download {key}: {s}: {body}")));
        }
        let info = ObjectInfo {
            size: parse_size(file.size.as_deref()),
            content_type: file
                .mime_type
                .clone()
                .or_else(|| Some(super::content_type_for(key).to_string())),
        };
        let stream = resp
            .bytes_stream()
            .map(|r| r.map_err(std::io::Error::other));
        Ok((info, Box::pin(stream)))
    }

    /// Granted "anyone with link can read" on the file (idempotent within process)
    /// and returned a public download URL. Used by `/redirect/...`: worker follows
    /// the 307 to Drive directly, byte traffic bypasses storage. Scope of the link
    /// is exactly one file, no expiration — to revoke, delete the file or its
    /// "anyone" permission via the Drive UI/API.
    pub async fn public_link(&self, key: &str) -> Result<String, BackendError> {
        let Some(file) = self.find_in_folder(&self.cfg.root_folder_id, key).await? else {
            return Err(BackendError::NotFound);
        };
        self.ensure_public(&file.id).await?;
        Ok(format!(
            "https://drive.google.com/uc?export=download&id={}",
            file.id
        ))
    }

    async fn ensure_public(&self, file_id: &str) -> Result<(), BackendError> {
        if self.public_files.lock().await.contains(file_id) {
            return Ok(());
        }
        let token = self.access_token().await?;
        let body = serde_json::json!({ "role": "reader", "type": "anyone" });
        let resp = self
            .http
            .post(format!(
                "{DRIVE_API}/files/{file_id}/permissions?supportsAllDrives=true"
            ))
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(http_err)?;
        if !resp.status().is_success() {
            let s = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(BackendError::Other(format!(
                "permissions.create {file_id}: {s}: {body}"
            )));
        }
        self.public_files.lock().await.insert(file_id.to_string());
        Ok(())
    }

    async fn find_in_folder(
        &self,
        parent: &str,
        name: &str,
    ) -> Result<Option<DriveFile>, BackendError> {
        let q = format!(
            "name='{}' and '{}' in parents and trashed=false",
            name, parent
        );
        self.list_first(&q).await
    }

    async fn list_first(&self, q: &str) -> Result<Option<DriveFile>, BackendError> {
        let token = self.access_token().await?;
        let mut url = url::Url::parse(&format!("{DRIVE_API}/files")).unwrap();
        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair("q", q);
            qp.append_pair("fields", "files(id,size,mimeType)");
            qp.append_pair("pageSize", "1");
            qp.append_pair("supportsAllDrives", "true");
            qp.append_pair("includeItemsFromAllDrives", "true");
            if let Some(drive_id) = &self.cfg.shared_drive_id {
                qp.append_pair("corpora", "drive");
                qp.append_pair("driveId", drive_id);
            }
        }
        let resp = self
            .http
            .get(url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(http_err)?;
        if !resp.status().is_success() {
            let s = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(BackendError::Other(format!("list files: {s}: {body}")));
        }
        let r: ListResp = resp.json().await.map_err(http_err)?;
        Ok(r.files.into_iter().next())
    }

    async fn upload_resumable(
        &self,
        parent_folder: &str,
        name: &str,
        mime: &str,
        src: &Path,
        existing_id: Option<&str>,
    ) -> Result<(), BackendError> {
        let meta = tokio::fs::metadata(src).await?;
        let total = meta.len();
        let token = self.access_token().await?;

        let metadata_json = if existing_id.is_some() {
            serde_json::json!({ "name": name, "mimeType": mime })
        } else {
            serde_json::json!({
                "name": name,
                "mimeType": mime,
                "parents": [parent_folder],
            })
        };

        let init = if let Some(id) = existing_id {
            self.http.patch(format!(
                "{DRIVE_UPLOAD_API}/files/{id}?uploadType=resumable&supportsAllDrives=true&fields=id"
            ))
        } else {
            self.http.post(format!(
                "{DRIVE_UPLOAD_API}/files?uploadType=resumable&supportsAllDrives=true&fields=id"
            ))
        };
        let init_resp = init
            .bearer_auth(&token)
            .header("X-Upload-Content-Type", mime)
            .header("X-Upload-Content-Length", total.to_string())
            .json(&metadata_json)
            .send()
            .await
            .map_err(http_err)?;

        if !init_resp.status().is_success() {
            let s = init_resp.status();
            let body = init_resp.text().await.unwrap_or_default();
            return Err(BackendError::Other(format!(
                "init resumable {name}: {s}: {body}"
            )));
        }

        let session_url = init_resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                BackendError::Other("init resumable: missing Location header".into())
            })?;

        let file = tokio::fs::File::open(src).await?;
        let stream = tokio_util::io::ReaderStream::new(file);
        let body = reqwest::Body::wrap_stream(stream);

        let put_resp = self
            .http
            .put(&session_url)
            .header(reqwest::header::CONTENT_TYPE, mime)
            .header(reqwest::header::CONTENT_LENGTH, total)
            .body(body)
            .send()
            .await
            .map_err(http_err)?;

        if !put_resp.status().is_success() {
            let s = put_resp.status();
            let body = put_resp.text().await.unwrap_or_default();
            return Err(BackendError::Other(format!(
                "upload bytes {name}: {s}: {body}"
            )));
        }

        Ok(())
    }

    async fn access_token(&self) -> Result<String, BackendError> {
        let mut g = self.token.lock().await;
        if let Some(cached) = &*g {
            if cached.expires_at > SystemTime::now() + TOKEN_REFRESH_LEEWAY {
                return Ok(cached.value.clone());
            }
        }
        let new = match &self.auth {
            AuthState::ServiceAccount(sa) => self.exchange_jwt(sa).await?,
            AuthState::UserOAuth {
                client_id,
                client_secret,
                refresh_token,
            } => self
                .exchange_refresh(client_id, client_secret, refresh_token)
                .await?,
        };
        let value = new.value.clone();
        *g = Some(new);
        Ok(value)
    }

    async fn exchange_jwt(&self, sa: &ServiceAccount) -> Result<CachedToken, BackendError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| BackendError::Other(format!("clock: {e}")))?
            .as_secs();
        let claims = JwtClaims {
            iss: &sa.client_email,
            scope: SCOPE,
            aud: TOKEN_URL,
            exp: now + 3600,
            iat: now,
        };
        let key = EncodingKey::from_rsa_pem(sa.private_key.as_bytes())
            .map_err(|e| BackendError::Other(format!("private key: {e}")))?;
        let assertion = encode(&Header::new(Algorithm::RS256), &claims, &key)
            .map_err(|e| BackendError::Other(format!("jwt sign: {e}")))?;

        let token_url = sa.token_uri.as_deref().unwrap_or(TOKEN_URL);
        let resp = self
            .http
            .post(token_url)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", assertion.as_str()),
            ])
            .send()
            .await
            .map_err(http_err)?;
        self.parse_token_resp(resp, "jwt").await
    }

    async fn exchange_refresh(
        &self,
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> Result<CachedToken, BackendError> {
        let resp = self
            .http
            .post(TOKEN_URL)
            .form(&[
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("refresh_token", refresh_token),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await
            .map_err(http_err)?;
        self.parse_token_resp(resp, "refresh").await
    }

    async fn parse_token_resp(
        &self,
        resp: reqwest::Response,
        kind: &str,
    ) -> Result<CachedToken, BackendError> {
        if !resp.status().is_success() {
            let s = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(BackendError::Other(format!(
                "token exchange ({kind}): {s}: {body}"
            )));
        }
        let tok: TokenResp = resp.json().await.map_err(http_err)?;
        let issued_at = SystemTime::now();
        Ok(CachedToken {
            value: tok.access_token,
            expires_at: issued_at + Duration::from_secs(tok.expires_in.saturating_sub(30)),
        })
    }
}

fn parse_size(s: Option<&str>) -> u64 {
    s.and_then(|s| s.parse().ok()).unwrap_or(0)
}

fn http_err(e: reqwest::Error) -> BackendError {
    BackendError::Other(format!("http: {e}"))
}
