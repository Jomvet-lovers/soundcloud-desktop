use std::env;

#[derive(Clone)]
pub struct Config {
    pub port: u16,
    pub database_host: String,
    pub database_port: u16,
    pub database_username: String,
    pub database_password: String,
    pub database_name: String,
    pub sc_proxy_url: String,
    pub sc_proxy_fallback: bool,
    pub sc_oauth_fallback_sessions: usize,
    pub sc_cookies: String,
    pub sc_oauth_token: Option<String>,
    pub premium_only: bool,
    pub storage_url: String,
    pub storage_public_url: String,
    pub storage_upload_url: String,
    pub storage_token: String,
    pub storage_cleanup_days: u64,
    pub storage_max_size_bytes: u64,
    pub storage_cleanup_interval_secs: u64,
    pub internal_token: String,
}

impl Config {
    pub fn from_env() -> Self {
        let cookies = env::var("SC_COOKIES").unwrap_or_default();
        let oauth_token = parse_cookie_value(&cookies, "oauth_token");

        let storage_url = env::var("STORAGE_URL").unwrap_or_default();
        let storage_public_url = env::var("STORAGE_PUBLIC_URL")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| storage_url.clone());
        let storage_upload_url = env::var("STORAGE_UPLOAD_URL")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| storage_url.clone());

        Self {
            port: env::var("PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8080),
            database_host: env::var("DATABASE_HOST").unwrap_or_else(|_| "localhost".into()),
            database_port: env::var("DATABASE_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5432),
            database_username: env::var("DATABASE_USERNAME")
                .unwrap_or_else(|_| "soundcloud".into()),
            database_password: env::var("DATABASE_PASSWORD")
                .unwrap_or_else(|_| "soundcloud".into()),
            database_name: env::var("DATABASE_NAME")
                .unwrap_or_else(|_| "soundcloud_desktop".into()),
            sc_proxy_url: env::var("SC_PROXY_URL").unwrap_or_default(),
            sc_proxy_fallback: env::var("SC_PROXY_FALLBACK")
                .map(|v| v == "true")
                .unwrap_or(false),
            sc_oauth_fallback_sessions: env::var("SC_OAUTH_FALLBACK_SESSIONS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
            sc_cookies: cookies,
            sc_oauth_token: oauth_token,
            premium_only: env::var("PREMIUM_ONLY")
                .map(|v| v == "true")
                .unwrap_or(false),
            storage_url,
            storage_public_url,
            storage_upload_url,
            storage_token: env::var("STORAGE_TOKEN").unwrap_or_default(),
            storage_cleanup_days: env::var("STORAGE_CLEANUP_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(7),
            storage_max_size_bytes: env::var("STORAGE_MAX_SIZE_BYTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            storage_cleanup_interval_secs: env::var("STORAGE_CLEANUP_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3600),
            internal_token: env::var("INTERNAL_TOKEN").unwrap_or_default(),
        }
    }

    pub fn storage_enabled(&self) -> bool {
        !self.storage_url.is_empty() && !self.storage_token.is_empty()
    }

    pub fn cookies_enabled(&self) -> bool {
        !self.sc_cookies.is_empty() && self.sc_oauth_token.is_some()
    }
}

fn parse_cookie_value(cookies: &str, name: &str) -> Option<String> {
    for part in cookies.split(';') {
        let part = part.trim();
        if let Some(idx) = part.find('=') {
            let key = part[..idx].trim();
            if key == name {
                let val = part[idx + 1..].trim();
                return Some(urlencoding_decode(val));
            }
        }
    }
    None
}

fn urlencoding_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().unwrap_or(b'0');
            let lo = chars.next().unwrap_or(b'0');
            let val = hex_digit(hi).unwrap_or(0) * 16 + hex_digit(lo).unwrap_or(0);
            result.push(val as char);
        } else {
            result.push(b as char);
        }
    }
    result
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}