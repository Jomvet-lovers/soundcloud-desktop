//! SC apiv2 methods authored as Lua, run via the relay.
//!
//! The business logic lives HERE (the backend), not in the relay — the relay is a
//! generic executor. Each script is embedded + validated at `cargo check` by
//! `lua_script!` (parse via full_moon + a forbidden-global lint), then handed to
//! `relay.call_method(method_id, SCRIPT, inputs)`. The `.lua` files live in
//! `backend/sc_methods/`. See `../../utils/call/lua-macros` and the call docs.

/// resolve a permalink URL → apiv2 track metadata.
pub const RESOLVE_TRACK: &str = call_lua_macros::lua_script!("sc_methods/resolve_track.lua");

/// apiv2 /tracks/{id} (full_duration recovery).
pub const TRACK_BY_ID: &str = call_lua_macros::lua_script!("sc_methods/track_by_id.lua");

/// apiv2 /users/{id} (token-free public profile).
pub const USER_BY_ID: &str = call_lua_macros::lua_script!("sc_methods/user_by_id.lua");

/// apiv2 playlist + its full ordered track list (batch /tracks?ids hydration) in one call.
pub const PLAYLIST_FULL: &str = call_lua_macros::lua_script!("sc_methods/playlist_full.lua");

/// apiv2 one page of a public per-user collection (likes/playlists/followings/tracks).
pub const USER_COLLECTION: &str = call_lua_macros::lua_script!("sc_methods/user_collection.lua");

/// apiv2 one page of a typed search (tracks/users/playlists/albums).
pub const SEARCH: &str = call_lua_macros::lua_script!("sc_methods/search.lua");
