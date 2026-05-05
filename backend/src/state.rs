use std::sync::Arc;

use sqlx::PgPool;

use crate::cache::{CacheService, ListCacheService};
use crate::config::AppConfig;
use crate::modules::auth::{AuthService, LinkService};
use crate::modules::collab::{CollabTrainerService, CollabVectorService};
use crate::modules::dislikes::DislikesService;
use crate::modules::events::EventsService;
use crate::modules::featured::FeaturedService;
use crate::modules::history::HistoryService;
use crate::modules::indexing::IndexingService;
use crate::modules::likes::LikesService;
use crate::modules::local_likes::LocalLikesService;
use crate::modules::ltr::LtrTrainerService;
use crate::modules::lyrics::LyricsService;
use crate::modules::me::MeService;
use crate::modules::oauth_apps::OAuthAppsService;
use crate::modules::pending_actions::PendingActionsService;
use crate::modules::playlists::PlaylistsService;
use crate::modules::recommendations::RecommendationsService;
use crate::modules::reposts::RepostsService;
use crate::modules::resolve::ResolveService;
use crate::modules::subscriptions::SubscriptionsService;
use crate::modules::tracks::TracksService;
use crate::modules::users::UsersService;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub pg: PgPool,
    pub cache: Arc<CacheService>,
    pub list_cache: Arc<ListCacheService>,
    pub auth: Arc<AuthService>,
    pub link: Arc<LinkService>,
    pub oauth_apps: Arc<OAuthAppsService>,
    pub local_likes: Arc<LocalLikesService>,
    pub events: Arc<EventsService>,
    pub dislikes: Arc<DislikesService>,
    pub subscriptions: Arc<SubscriptionsService>,
    pub me: Arc<MeService>,
    pub pending_actions: Arc<PendingActionsService>,
    pub tracks: Arc<TracksService>,
    pub playlists: Arc<PlaylistsService>,
    pub users: Arc<UsersService>,
    pub likes: Arc<LikesService>,
    pub reposts: Arc<RepostsService>,
    pub resolve: Arc<ResolveService>,
    pub history: Arc<HistoryService>,
    pub featured: Arc<FeaturedService>,
    pub lyrics: Arc<LyricsService>,
    pub collab_vector: Arc<CollabVectorService>,
    pub collab_trainer: Arc<CollabTrainerService>,
    pub ltr_trainer: Arc<LtrTrainerService>,
    pub indexing: Arc<IndexingService>,
    pub recommendations: Arc<RecommendationsService>,
}
