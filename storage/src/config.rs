use std::env;
use std::path::PathBuf;

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
    /// Корень рабочего tmp. Внутри держим `source/` (сырые аплоады)
    /// и `result/` (выходы ffmpeg, ждущие отгрузки в backend).
    pub tmp_path: String,
    pub ffmpeg_bin: String,
    pub ffprobe_bin: String,
    /// Макс. одновременных ffmpeg-процессов. Каждый процесс обрабатывает
    /// батч из `transcode_batch_size` треков за один вызов.
    pub max_transcodes: usize,
    /// Сколько треков склеиваем в один ffmpeg-вызов.
    pub transcode_batch_size: usize,
    /// Сколько ждём добор батча, прежде чем стрелять ffmpeg-ом.
    pub transcode_batch_wait_ms: u64,
    /// Параллельные backend-загрузки (S3 PUT / local rename).
    pub upload_concurrency: usize,
    /// Сколько раз ретраить одну backend-загрузку (на ошибки).
    pub upload_retries: usize,
    /// База exponential backoff между ретраями загрузки.
    pub upload_retry_base_ms: u64,
    /// Лимит общего размера source-каталога. None = без лимита.
    pub tmp_max_bytes: Option<u64>,
    /// Если true — `/upload` отдаёт 503. Для хостов-раздатчиков без ffmpeg.
    pub disable_upload: bool,
    pub backend: BackendKind,
    pub s3: Option<S3Config>,
    /// JetStream URL для публикации `storage.track_uploaded`. Пусто = no-op.
    pub nats_url: String,
    /// Базовый URL для composing redirect-ссылок в payload события.
    /// Должен совпадать с `STORAGE_PUBLIC_URL` в стриминге/бэке (обычно публичный домен сервиса).
    /// Пусто = публикация события skip'ается (нет читаемого URL для воркера).
    pub event_base_url: String,
}

impl Config {
    pub fn source_path(&self) -> String {
        let p: PathBuf = PathBuf::from(&self.tmp_path).join("source");
        p.to_string_lossy().into_owned()
    }

    pub fn result_path(&self) -> String {
        let p: PathBuf = PathBuf::from(&self.tmp_path).join("result");
        p.to_string_lossy().into_owned()
    }
}

fn parse_bool(v: &str) -> bool {
    matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

fn parse_env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(default)
}

fn parse_env_u64(name: &str, default: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(default)
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
                    .map(|v| parse_bool(&v))
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
            tmp_path: env::var("TMP_PATH").unwrap_or_else(|_| "/tmp/storage".into()),
            ffmpeg_bin: env::var("FFMPEG_BIN").unwrap_or_else(|_| "ffmpeg".into()),
            ffprobe_bin: env::var("FFPROBE_BIN").unwrap_or_else(|_| "ffprobe".into()),
            max_transcodes: parse_env_usize(
                "MAX_TRANSCODES",
                std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(2),
            ),
            transcode_batch_size: parse_env_usize("TRANSCODE_BATCH_SIZE", 8),
            transcode_batch_wait_ms: parse_env_u64("TRANSCODE_BATCH_WAIT_MS", 50),
            upload_concurrency: parse_env_usize("UPLOAD_CONCURRENCY", 64),
            upload_retries: parse_env_usize("UPLOAD_RETRIES", 4),
            upload_retry_base_ms: parse_env_u64("UPLOAD_RETRY_BASE_MS", 250),
            tmp_max_bytes: env::var("TMP_MAX_GB")
                .ok()
                .and_then(|v| v.trim().parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v > 0.0)
                .map(|gb| {
                    let bytes = gb * 1024.0 * 1024.0 * 1024.0;
                    if bytes >= u64::MAX as f64 {
                        u64::MAX
                    } else {
                        bytes as u64
                    }
                }),
            disable_upload: env::var("DISABLE_UPLOAD")
                .ok()
                .map(|v| parse_bool(&v))
                .unwrap_or(false),
            backend,
            s3,
            nats_url: env::var("NATS_URL").unwrap_or_default(),
            event_base_url: env::var("EVENT_BASE_URL")
                .unwrap_or_default()
                .trim_end_matches('/')
                .to_string(),
        }
    }
}
