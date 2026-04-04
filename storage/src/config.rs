use std::env;

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
}

impl Config {
    pub fn from_env() -> Self {
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
        }
    }
}
