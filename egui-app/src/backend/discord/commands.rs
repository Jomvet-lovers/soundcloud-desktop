use std::sync::{Arc, Mutex};

use discord_rich_presence::{
    activity::{Activity, ActivityType, Assets, Button, StatusDisplayType, Timestamps},
    DiscordIpc, DiscordIpcClient,
};

use crate::backend::shared::constants::{DISCORD_CLIENT_ID, GITHUB_URL};

/// Discord rejects empty (or invisible-only) details/state values, requires
/// at least 2 characters for each, and caps them at 128 characters.
/// A failed `set_activity` leaves the previous presence on the profile, so
/// tracks without metadata used to freeze the widget on the song played
/// before them. Our sender is fire-and-forget (Discord answers on the pipe,
/// which nobody reads), so such rejections are invisible — e.g. an uploader
/// literally named "-" froze the widget with no error anywhere. Treat fewer
/// than 2 visible chars as missing so the fallbacks below apply.
const FIELD_MAX_CHARS: usize = 128;
const FIELD_MIN_VISIBLE_CHARS: usize = 2;

fn clean_field(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    let visible = trimmed
        .chars()
        .filter(|c| {
            !c.is_whitespace()
                && !matches!(
                    c,
                    '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{3164}' | '\u{FEFF}'
                )
        })
        .take(FIELD_MIN_VISIBLE_CHARS + 1)
        .count();
    if visible >= FIELD_MIN_VISIBLE_CHARS {
        Some(trimmed)
    } else {
        None
    }
}

fn clamp_field(value: &str) -> String {
    if value.chars().count() <= FIELD_MAX_CHARS {
        return value.to_string();
    }
    let mut out: String = value.chars().take(FIELD_MAX_CHARS - 1).collect();
    out.push('…');
    out
}

pub struct DiscordState {
    pub client: Mutex<Option<DiscordIpcClient>>,
}

#[derive(serde::Deserialize)]
pub struct DiscordTrackInfo {
    pub title: String,
    pub artist: String,
    pub artwork_url: Option<String>,
    pub track_url: Option<String>,
    pub artist_url: Option<String>,
    pub duration_secs: Option<i64>,
    pub elapsed_secs: Option<i64>,
    pub is_playing: Option<bool>,
    pub mode: Option<DiscordRpcMode>,
    pub show_button: Option<bool>,
}

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscordRpcMode {
    Track,
    Artist,
    Activity,
}

pub fn discord_connect(state: &Arc<DiscordState>) -> Result<bool, String> {
    let mut guard = state.client.lock().map_err(|e| e.to_string())?;
    if guard.is_some() {
        return Ok(true);
    }
    let mut client = DiscordIpcClient::new(DISCORD_CLIENT_ID);
    match client.connect() {
        Ok(_) => {
            println!("[Discord] Connected");
            *guard = Some(client);
            Ok(true)
        }
        Err(e) => {
            println!("[Discord] Connection failed: {e}");
            Err(format!("Connection failed: {e}"))
        }
    }
}

pub fn discord_disconnect(state: &Arc<DiscordState>) {
    let Ok(mut guard) = state.client.lock() else {
        return;
    };
    if let Some(ref mut client) = *guard {
        let _ = client.close();
        println!("[Discord] Disconnected");
    }
    *guard = None;
}

pub fn discord_set_activity(
    state: &Arc<DiscordState>,
    track: DiscordTrackInfo,
) -> Result<(), String> {
    let mut guard = state.client.lock().map_err(|e| e.to_string())?;
    let client = guard.as_mut().ok_or("Discord not connected")?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let elapsed = track.elapsed_secs.unwrap_or(0);
    let start = now - elapsed;
    let is_playing = track.is_playing.unwrap_or(true);
    let mode = track.mode.unwrap_or(DiscordRpcMode::Track);
    let show_button = track.show_button.unwrap_or(true);

    let title = clamp_field(clean_field(&track.title).unwrap_or("Untitled"));
    let artist = clamp_field(clean_field(&track.artist).unwrap_or("Unknown Artist"));

    let track_url = track
        .track_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty());
    let artist_url = track
        .artist_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty());

    let large_image = track.artwork_url.as_deref().unwrap_or("soundcloud_logo");

    // NOTE: `large_text` is deliberately not set — Discord renders it as an
    // extra line in the activity card (the "album" slot next to details and
    // state). `large_url` makes the cover image clickable (track page).
    let mut assets = Assets::new().large_image(large_image);
    if let Some(url) = track_url {
        assets = assets.large_url(url);
    }

    // `status_display_type` picks what the member list shows next to the
    // activity type: the artist (Spotify-like), the title, or the app name.
    let mut activity = Activity::new()
        .activity_type(ActivityType::Listening)
        .assets(assets)
        .status_display_type(match mode {
            DiscordRpcMode::Track => StatusDisplayType::State,
            DiscordRpcMode::Artist => StatusDisplayType::Details,
            DiscordRpcMode::Activity => StatusDisplayType::Name,
        });

    match mode {
        DiscordRpcMode::Track => {
            activity = activity.details(&title);
            if let Some(url) = track_url {
                activity = activity.details_url(url);
            }
            activity = activity.state(&artist);
            if let Some(url) = artist_url {
                activity = activity.state_url(url);
            }
        }
        DiscordRpcMode::Artist => {
            activity = activity.details(&artist);
            if let Some(url) = artist_url {
                activity = activity.details_url(url);
            }
        }
        DiscordRpcMode::Activity => {}
    }

    if is_playing {
        let mut timestamps = Timestamps::new().start(start);
        if let Some(dur) = track.duration_secs {
            timestamps = timestamps.end(start + dur);
        }
        activity = activity.timestamps(timestamps);
    }

    if show_button {
        activity = activity.buttons(vec![Button::new("GitHub", GITHUB_URL)]);
    }

    let result = client.set_activity(activity);

    if result.is_err() {
        *guard = None;
    }

    result.map_err(|e| format!("set_activity: {e}"))?;

    Ok(())
}

pub fn discord_clear_activity(state: &Arc<DiscordState>) -> Result<(), String> {
    let mut guard = state.client.lock().map_err(|e| e.to_string())?;
    if let Some(ref mut client) = *guard {
        client
            .clear_activity()
            .map_err(|e| format!("clear_activity: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::clean_field;

    /// Regression: an uploader literally named "-" produced `state: "-"`,
    /// which Discord rejects (min 2 chars, IPC ERROR 4000), freezing the
    /// widget on the previous track with no error anywhere.
    #[test]
    fn single_char_fields_fall_back() {
        assert_eq!(clean_field("-"), None);
        assert_eq!(clean_field("  -  "), None);
        // Single CJK char hits the same Discord rule.
        assert_eq!(clean_field("愛"), None);
        assert_eq!(clean_field(""), None);
        assert_eq!(clean_field("   "), None);
    }

    #[test]
    fn normal_fields_pass_through_trimmed() {
        assert_eq!(clean_field("  S0S  "), Some("S0S"));
        assert_eq!(clean_field("ETIA."), Some("ETIA."));
    }
}
