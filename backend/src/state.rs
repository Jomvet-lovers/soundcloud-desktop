use std::sync::Arc;

use sqlx::PgPool;

use crate::bus::nats::NatsService;
use crate::cache::{CacheService, ListCacheService};
use crate::config::AppConfig;
use crate::modules::auth::{AuthService, LinkService};
use crate::modules::centroids::CentroidService;
use crate::modules::collab::{CollabTrainerService, CollabVectorService};
use crate::modules::dislikes::DislikesService;
use crate::modules::events::EventsService;
use crate::modules::featured::FeaturedService;
use crate::modules::history::HistoryService;
use crate::modules::indexing::IndexingService;
use crate::modules::likes::LikesService;
use crate::modules::local_likes::LocalLikesService;
use crate::modules::ltr::{LtrService, LtrTrainerService};
use crate::modules::lyrics::{LyricsService, WorkerClient};
use crate::modules::me::MeService;
use crate::modules::oauth_apps::OAuthAppsService;
use crate::modules::pending_actions::PendingActionsService;
use crate::modules::playlists::PlaylistsService;
use crate::modules::recommendations::{RecommendationsService, S3VerifierService};
use crate::modules::reposts::RepostsService;
use crate::modules::resolve::ResolveService;
use crate::modules::subscriptions::SubscriptionsService;
use crate::modules::tracks::TracksService;
use crate::modules::transcode::TranscodeTriggerService;
use crate::modules::user_taste::UserTasteService;
use crate::modules::users::UsersService;
use crate::qdrant::QdrantService;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub pg: PgPool,
    pub cache: Arc<CacheService>,
    pub list_cache: Arc<ListCacheService>,
    pub nats: Arc<NatsService>,
    pub qdrant: Arc<QdrantService>,
    pub http: reqwest::Client,
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
    pub centroids: Arc<CentroidService>,
    pub user_taste: Arc<UserTasteService>,
    pub transcode: Arc<TranscodeTriggerService>,
    pub lyrics: Arc<LyricsService>,
    pub worker: Arc<WorkerClient>,
    pub collab_vector: Arc<CollabVectorService>,
    pub collab_trainer: Arc<CollabTrainerService>,
    pub ltr: Arc<LtrService>,
    pub ltr_trainer: Arc<LtrTrainerService>,
    pub indexing: Arc<IndexingService>,
    pub s3_verifier: Arc<S3VerifierService>,
    pub recommendations: Arc<RecommendationsService>,
}
