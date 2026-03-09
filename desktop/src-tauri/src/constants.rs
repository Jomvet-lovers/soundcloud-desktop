pub const DISCORD_CLIENT_ID: &str = "1431978756687265872";

pub const PROXY_URL: &str = "https://soundcloud.work.gd";
pub const DOMAIN_WHITELIST: &[&str] = &[
    "localhost",
    "127.0.0.1",
    "tauri.localhost",
    "backend.soundcloud.work.gd",
    "soundcloud.work.gd",
];

pub fn is_domain_whitelisted(host: &str) -> bool {
    DOMAIN_WHITELIST.iter().any(|&w| host == w)
}
