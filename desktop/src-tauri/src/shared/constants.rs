pub const DISCORD_CLIENT_ID: &str = "1431978756687265872";

pub const DOMAIN_WHITELIST: &[&str] = &[
    "localhost",
    "127.0.0.1",
    "tauri.localhost",
    "api.soundcloud.su",
    "images.soundcloud.su",
    "stream.soundcloud.su",
    "api.scdinternal.site",
    "images.scdinternal.site",
    "storage.scdinternal.site",
    "white.storage.scdinternal.site",
    "stream.scdinternal.site",
    "stream-premium.scdinternal.site",
    "white.api.scdinternal.site",
    "white.images.scdinternal.site",
    "white.stream.scdinternal.site",
    "white.stream-premium.scdinternal.site",
];

pub fn is_domain_whitelisted(host: &str) -> bool {
    DOMAIN_WHITELIST.iter().any(|&w| host == w)
}
