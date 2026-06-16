//! SC audio-fetch methods authored as Lua, run via the relay.
//!
//! The audio business logic lives HERE (the streaming service), not in the relay.
//! Each script is embedded + validated at `cargo check` by `lua_script!` and handed
//! to `relay.call_method(method_id, SCRIPT, inputs)`. Files: `streaming/sc_methods/`.

/// resolve an apiv2 transcoding URL → signed CDN url via the relay.
pub const TRANSCODING_RESOLVE: &str =
    call_lua_macros::lua_script!("sc_methods/transcoding_resolve.lua");

/// "give me the track" — one-shot: the relay does the whole flow (metadata →
/// pick transcoding → resolve → download/decrypt) and returns the audio. Primary path.
pub const GET_TRACK: &str = call_lua_macros::lua_script!("sc_methods/get_track.lua");

/// download a progressive (single-file) track via the relay; returns base64 audio.
pub const PROGRESSIVE_DOWNLOAD: &str =
    call_lua_macros::lua_script!("sc_methods/progressive_download.lua");

/// download + glue an hls track via the relay; returns base64 audio (mode B).
pub const HLS_DOWNLOAD: &str = call_lua_macros::lua_script!("sc_methods/hls_download.lua");

/// decrypt a ctr-encrypted-hls (Widevine) track via the relay (it fetches a
/// served .wvd device); returns base64 fMP4.
pub const HLS_DECRYPT: &str = call_lua_macros::lua_script!("sc_methods/hls_decrypt.lua");
