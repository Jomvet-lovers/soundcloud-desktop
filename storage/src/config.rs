use std::env;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendKind {
    Local,
    S3,
}

#[derive(Clone, Debug)]
pub struct S3Config {
    pub endpoint: Option<String>,
    pub region: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub force_path_style: bool,
}

#[derive(Clone)]
pub struct Config {
    pub port: u16,
    pub admin_token: String,
    pub storage_path: String,
    pub tmp_path: String,
    pub ffmpeg_bin: String,
    pub ffprobe_bin: String,
    /// Max concurrent ffmpeg transcodes
    pub max_transcodes: usize,
    pub backend: BackendKind,
    pub s3: Option<S3Config>,
}

impl Config {
    pub fn from_env() -> Self {
        let backend = match env::var("STORAGE_BACKEND")
            .unwrap_or_else(|_| "local".into())
            .to_ascii_lowercase()
            .as_str()
        {
            "s3" => BackendKind::S3,
            "local" | "" => BackendKind::Local,
            other => panic!("unknown STORAGE_BACKEND: {other} (expected 'local' or 's3')"),
        };

        let s3 = if backend == BackendKind::S3 {
            Some(S3Config {
                endpoint: env::var("S3_ENDPOINT").ok().filter(|v| !v.is_empty()),
                region: env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".into()),
                bucket: env::var("S3_BUCKET").expect("S3_BUCKET is required for s3 backend"),
                access_key_id: env::var("S3_ACCESS_KEY_ID")
                    .expect("S3_ACCESS_KEY_ID is required for s3 backend"),
                secret_access_key: env::var("S3_SECRET_ACCESS_KEY")
                    .expect("S3_SECRET_ACCESS_KEY is required for s3 backend"),
                force_path_style: env::var("S3_FORCE_PATH_STYLE")
                    .ok()
                    .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes"))
                    .unwrap_or(true),
            })
        } else {
            None
        };

        Self {
            port: env::var("PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3000),
            admin_token: env::var("ADMIN_TOKEN").expect("ADMIN_TOKEN is required"),
            storage_path: env::var("STORAGE_PATH").unwrap_or_else(|_| "/data/storage".into()),
            tmp_path: env::var("TMP_PATH").unwrap_or_else(|_| "/tmp/transcode".into()),
            ffmpeg_bin: env::var("FFMPEG_BIN").unwrap_or_else(|_| "ffmpeg".into()),
            ffprobe_bin: env::var("FFPROBE_BIN").unwrap_or_else(|_| "ffprobe".into()),
            max_transcodes: env::var("MAX_TRANSCODES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| {
                    std::thread::available_parallelism()
                        .map(|n| n.get())
                        .unwrap_or(2)
                }),
            backend,
            s3,
        }
    }
}
