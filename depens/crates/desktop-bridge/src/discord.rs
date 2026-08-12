//! C-ABI Discord Rich Presence: оболочка драйвит присутствие по медиа-хукам
//! движка (тот же now_playing/set_playing/clear, что и у MPRIS).

use std::ffi::c_char;
use std::sync::OnceLock;

use discord_rpc::DiscordPresence;

use crate::cstr;

const DISCORD_CLIENT_ID: &str = "1431978756687265872";
static DISCORD: OnceLock<DiscordPresence> = OnceLock::new();

/// Подключиться к Discord IPC. `false` — Discord не запущен (всё равно безопасно).
#[unsafe(no_mangle)]
pub extern "C" fn sc_discord_init() -> bool {
    DISCORD
        .get_or_init(|| DiscordPresence::new(DISCORD_CLIENT_ID))
        .connect()
}

/// Новый трек: title/artist + обложка + ссылка + длительность (сек).
/// # Safety
/// `title`/`artist`/`cover_url`/`track_url` — валидные NUL-строки либо null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sc_discord_now_playing(
    title: *const c_char,
    artist: *const c_char,
    cover_url: *const c_char,
    track_url: *const c_char,
    duration_secs: i64,
) {
    if let Some(discord) = DISCORD.get() {
        let title = unsafe { cstr(title) };
        let artist = unsafe { cstr(artist) };
        let cover = unsafe { cstr(cover_url) };
        let url = unsafe { cstr(track_url) };
        discord.set_now_playing(&title, &artist, &cover, &url, duration_secs);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn sc_discord_set_playing(playing: bool) {
    if let Some(discord) = DISCORD.get() {
        discord.set_playing(playing);
    }
}

/// Тик позиции (сек) — для пересчёта таймстемпов на следующем play/pause.
#[unsafe(no_mangle)]
pub extern "C" fn sc_discord_set_position(position_secs: i64) {
    if let Some(discord) = DISCORD.get() {
        discord.set_position(position_secs);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn sc_discord_clear() {
    if let Some(discord) = DISCORD.get() {
        discord.clear();
    }
}
