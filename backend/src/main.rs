#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

mod bus;
mod cache;
mod common;
mod config;
mod db;
mod error;
mod modules;
mod qdrant;
mod redis;
mod router;
mod sc;
mod state;
mod telemetry;

use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::bus::nats::NatsService;
use crate::cache::{CacheService, ListCacheService};
use crate::config::AppConfig;
use crate::modules::auth::{AuthService, LinkService};
use crate::modules::centroids::CentroidService;
use crate::modules::collab::{CollabTrainerService, CollabVectorService};
use crate::modules::dislikes::DislikesService;
use crate::modules::ltr::{LtrService, LtrTrainerService};
use crate::modules::events::EventsService;
use crate::modules::featured::FeaturedService;
use crate::modules::history::HistoryService;
use crate::modules::indexing::IndexingService;
use crate::modules::likes::LikesService;
use crate::modules::local_likes::LocalLikesService;
use crate::modules::lyrics::genius::GeniusService;
use crate::modules::lyrics::lrclib::LrclibService;
use crate::modules::lyrics::musixmatch::MusixmatchService;
use crate::modules::lyrics::netease::NeteaseService;
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
use crate::sc::ScClient;
use crate::state::AppState;

const BG_TICK: Duration = Duration::from_secs(60);
const BG_WORK_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    telemetry::init();

    let config = Arc::new(AppConfig::from_env());
    info!(port = config.port, "backend starting");

    let pg = db::connect(&config).await.expect("Failed to connect to PostgreSQL");
    info!("PostgreSQL connected");
    if let Err(e) = db::migrate(&pg).await {
        error!(error = %e, "Failed to run migrations");
        std::process::exit(1);
    }
    info!("Migrations applied");

    let redis_pool = redis::connect(&config).expect("Failed to create Redis pool");
    info!("Redis pool ready");

    let shutdown = CancellationToken::new();

    let nats = NatsService::connect(&config.nats.url, shutdown.clone())
        .await
        .expect("Failed to connect to NATS");
    info!("NATS connected");

    let qdrant = QdrantService::connect(&config.qdrant).expect("Failed to init Qdrant client");
    qdrant.bootstrap_collections().await;
    info!("Qdrant client ready");

    let http_client = reqwest::Client::builder()
        .tcp_keepalive(Duration::from_secs(60))
        .pool_max_idle_per_host(20)
        .pool_idle_timeout(Duration::from_secs(90))
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .user_agent("scd-backend/0.1")
        .build()
        .expect("Failed to build shared HTTP client");

    let sc = ScClient::new(&config.soundcloud).expect("Failed to build SC HTTP client");

    let oauth_apps = OAuthAppsService::new(pg.clone(), config.clone());
    if let Err(e) = oauth_apps.migrate_env_app().await {
        warn!(error = %e, "OAuthApps env migration failed");
    }
    match oauth_apps.count_active().await {
        Ok(n) => info!(active = n, "Active OAuth apps"),
        Err(e) => warn!(error = %e, "Failed to count active OAuth apps"),
    }

    let auth = AuthService::new(pg.clone(), sc.clone(), oauth_apps.clone(), config.clone());
    let link = LinkService::new(pg.clone(), auth.clone());

    let cache = CacheService::new(redis_pool.clone());
    let list_cache = ListCacheService::new(redis_pool.clone());
    let local_likes = LocalLikesService::new(pg.clone());
    let events = EventsService::new(pg.clone());
    let subscriptions = SubscriptionsService::new(pg.clone(), config.subscriptions.snapshot_dir.clone());
    if let Err(e) = subscriptions.restore_from_snapshot().await {
        warn!(error = %e, "subscriptions restore failed");
    }
    subscriptions.spawn_snapshot_loop(shutdown.clone());
    let me = MeService::new(sc.clone(), list_cache.clone(), local_likes.clone(), events.clone());
    let pending_actions = PendingActionsService::new(pg.clone(), sc.clone(), auth.clone(), oauth_apps.clone());
    let tracks = TracksService::new(sc.clone(), list_cache.clone(), local_likes.clone(), pending_actions.clone());
    let playlists = PlaylistsService::new(sc.clone(), list_cache.clone(), pending_actions.clone());
    let users = UsersService::new(sc.clone(), list_cache.clone(), local_likes.clone());
    let dislikes = DislikesService::new(pg.clone(), events.clone());
    let likes = LikesService::new(sc.clone(), local_likes.clone(), pending_actions.clone());
    let reposts = RepostsService::new(sc.clone(), pending_actions.clone());
    let resolve = ResolveService::new(sc.clone(), pg.clone());
    let history = HistoryService::new(pg.clone());
    let featured = FeaturedService::new(pg.clone(), sc.clone(), auth.clone(), local_likes.clone());
    let centroids = CentroidService::new(qdrant.clone());
    let user_taste = UserTasteService::new(qdrant.clone());
    let transcode = TranscodeTriggerService::new(http_client.clone(), config.clone(), nats.clone());
    let worker = WorkerClient::new(nats.clone());
    let lrclib = LrclibService::new(http_client.clone());
    let mxm = MusixmatchService::new(http_client.clone(), config.mxm.api_base.clone());
    let genius = GeniusService::new(http_client.clone());
    let netease = NeteaseService::new(http_client.clone(), config.netease.api_base.clone());
    let lyrics = LyricsService::new(
        pg.clone(),
        nats.clone(),
        lrclib,
        mxm,
        genius,
        netease,
        worker.clone(),
        transcode.clone(),
        config.lyrics.indexing_concurrency,
    );
    lyrics.spawn_consumers();
    lyrics.spawn_reap_loops(shutdown.clone());

    centroids.spawn_refresh_loop(shutdown.clone());

    let collab_vector = CollabVectorService::new(qdrant.clone(), pg.clone());
    let collab_trainer = CollabTrainerService::new(
        pg.clone(),
        nats.clone(),
        collab_vector.clone(),
        config.collab.clone(),
    );
    collab_trainer.spawn_bootstrap_and_cron(shutdown.clone());

    let ltr = LtrService::new(nats.clone(), config.ltr.clone());
    let ltr_trainer = LtrTrainerService::new(
        pg.clone(),
        qdrant.clone(),
        collab_vector.clone(),
        centroids.clone(),
        ltr.clone(),
        config.ltr.clone(),
    );
    ltr_trainer.spawn_bootstrap_and_cron(shutdown.clone());

    let indexing = IndexingService::new(
        pg.clone(),
        nats.clone(),
        lyrics.clone(),
        transcode.clone(),
    );
    indexing.spawn(shutdown.clone());

    let track_discovery = crate::modules::indexing::TrackDiscoveryService::new(
        sc.clone(),
        indexing.clone(),
    );
    sc.install_track_observer(track_discovery.clone());

    let s3_verifier = S3VerifierService::new(http_client.clone(), config.storage.url.clone(), pg.clone());
    let recommendations = RecommendationsService::new(
        qdrant.clone(),
        pg.clone(),
        worker.clone(),
        s3_verifier.clone(),
        centroids.clone(),
        collab_vector.clone(),
        ltr.clone(),
        config.soundwave.clone(),
    );

    events.install_deps(
        user_taste.clone(),
        indexing.clone(),
        dislikes.clone(),
        collab_vector.clone(),
        collab_trainer.clone(),
    );
    events.spawn_indexing_queue_consumer(nats.clone());

    let mut tasks = JoinSet::new();

    {
        let token = shutdown.clone();
        let pa = pending_actions.clone();
        tasks.spawn(async move {
            run_periodic("pending_actions.sync_all", token, BG_TICK, BG_WORK_TIMEOUT, move || {
                let pa = pa.clone();
                async move { pa.sync_all().await.map(|_| ()) }
            })
            .await;
        });
    }

    {
        let token = shutdown.clone();
        let auth = auth.clone();
        tasks.spawn(async move {
            run_periodic("auth.cleanup_login_requests", token, BG_TICK, BG_WORK_TIMEOUT, move || {
                let auth = auth.clone();
                async move { auth.cleanup_expired_login_requests().await }
            })
            .await;
        });
    }

    {
        let token = shutdown.clone();
        let auth = auth.clone();
        tasks.spawn(async move {
            run_periodic("auth.cleanup_link_requests", token, BG_TICK, BG_WORK_TIMEOUT, move || {
                let auth = auth.clone();
                async move { auth.cleanup_expired_link_requests().await }
            })
            .await;
        });
    }

    let port = config.port;
    let state = AppState {
        config: config.clone(),
        pg,
        cache,
        list_cache,
        nats,
        qdrant,
        http: http_client,
        auth,
        link,
        oauth_apps,
        local_likes,
        events,
        dislikes,
        subscriptions,
        me,
        pending_actions,
        tracks,
        playlists,
        users,
        likes,
        reposts,
        resolve,
        history,
        featured,
        centroids,
        user_taste,
        transcode,
        lyrics,
        worker,
        collab_vector,
        collab_trainer,
        ltr,
        ltr_trainer,
        indexing,
        s3_verifier,
        recommendations,
    };

    let app = router::build(state);

    if let Some(tls_cfg) = tls_common::TlsConfig::from_env() {
        info!("starting with TLS (ACME)");
        tls_common::serve(tls_cfg, app).await;
    } else {
        let addr = format!("0.0.0.0:{port}");
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .expect("Failed to bind");
        info!(%addr, "starting plain HTTP");
        axum::serve(listener, app)
            .with_graceful_shutdown(tls_common::shutdown_signal())
            .await
            .expect("Server error");
    }

    shutdown.cancel();
    while tasks.join_next().await.is_some() {}
    info!("backend stopped");
}

async fn run_periodic<F, Fut>(
    name: &'static str,
    token: CancellationToken,
    tick: Duration,
    work_timeout: Duration,
    make_fut: F,
) where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = error::AppResult<()>>,
{
    let mut ticker = tokio::time::interval(tick);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = token.cancelled() => break,
            _ = ticker.tick() => {
                match tokio::time::timeout(work_timeout, make_fut()).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => warn!(task = name, error = %e, "Background task failed"),
                    Err(_) => warn!(task = name, timeout_secs = work_timeout.as_secs(), "Background task timed out"),
                }
            }
        }
    }
}
