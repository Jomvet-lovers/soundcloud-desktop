pub mod auth_overview;
pub mod catalog;
pub mod infra;
pub mod maintenance;
pub mod stats;
pub mod sync_queue;
pub mod wanted;

use axum::routing::{delete, get, patch, post};
use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/stats", get(stats::get_stats))
        .route("/admin/sync-queue", get(sync_queue::get_stats))
        .route("/admin/sync-queue/flush", post(sync_queue::flush))
        .route("/admin/sync-queue/purge", post(sync_queue::purge))
        .route("/admin/auth/overview", get(auth_overview::overview))
        .route("/admin/oauth-apps/health", get(auth_overview::oauth_health))
        .route("/admin/infra", get(infra::get_infra))
        .route("/admin/http-stats", get(infra::http_stats))
        .route("/admin/slow-queries", get(infra::slow_queries))
        .route("/admin/index-usage", get(infra::index_usage))
        .route(
            "/admin/maintenance/renormalize",
            post(maintenance::renormalize),
        )
        .route(
            "/admin/maintenance/mb-artist-names",
            post(maintenance::mb_artist_names),
        )
        .route("/admin/wanted-tracks", get(wanted::list))
        .route("/admin/wanted-tracks/{id}/link", post(wanted::link))
        .route(
            "/admin/wanted-tracks/{id}/status",
            patch(wanted::set_status),
        )
        // catalog management
        .route("/admin/resolve", get(catalog::resolve))
        .route(
            "/admin/artists",
            get(catalog::artists_search).post(catalog::artist_create),
        )
        .route(
            "/admin/artists/{artist_id}",
            get(catalog::artist_detail).patch(catalog::artist_update),
        )
        .route(
            "/admin/artists/{artist_id}/tracks",
            get(catalog::artist_tracks),
        )
        .route(
            "/admin/artists/{artist_id}/sc-accounts/{sc_user_id}/tracks",
            get(catalog::sc_account_tracks),
        )
        .route(
            "/admin/artists/{artist_id}/sc-accounts/{sc_user_id}/detach-tracks",
            post(catalog::sc_account_detach_tracks),
        )
        .route("/admin/albums", get(catalog::albums_search))
        .route("/admin/tracks", get(catalog::tracks_search))
        .route("/admin/tracks/{track_id}", get(catalog::track_detail))
        .route(
            "/admin/tracks/{track_id}/primary-artist",
            patch(catalog::track_set_primary_artist),
        )
        .route(
            "/admin/tracks/{track_id}/album",
            patch(catalog::track_set_album),
        )
        .route(
            "/admin/tracks/{track_id}/credits",
            post(catalog::track_add_credit),
        )
        .route(
            "/admin/tracks/{track_id}/credits/{artist_id}",
            delete(catalog::track_remove_credit),
        )
        .route(
            "/admin/tracks/{track_id}/detach-artist",
            post(catalog::track_detach_artist),
        )
        .route(
            "/admin/tracks/{track_id}/blocks/{artist_id}",
            delete(catalog::track_unblock_artist),
        )
}
