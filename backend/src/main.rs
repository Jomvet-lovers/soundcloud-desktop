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
use crate::modules::auras::AurasService;
use crate::modules::auth::{AuthService, LinkService, TokenProvider};
use crate::modules::cold_refresh::ColdRefreshService;
use crate::modules::collab::{CollabTrainerService, CollabVectorService};
use crate::modules::discover::DiscoverService;
use crate::modules::dislikes::DislikesService;
use crate::modules::enrich::{AiResolverClient, ArtistCrawlService, EnrichService, MbClient};
use crate::modules::events::EventsService;
use crate::modules::featured::FeaturedService;
use crate::modules::history::HistoryService;
use crate::modules::indexing::IndexingService;
use crate::modules::likes::LikesService;
use crate::modules::lyrics::genius::GeniusService;
use crate::modules::lyrics::lrclib::LrclibService;
use crate::modules::lyrics::musixmatch::MusixmatchService;
use crate::modules::lyrics::netease::NeteaseService;
use crate::modules::lyrics::{LyricsService, WorkerClient};
use crate::modules::me::MeService;
use crate::modules::oauth_apps::{OAuthAppTokenService, OAuthAppsService};
use crate::modules::playlists::PlaylistsService;
use crate::modules::recommendations::{RecommendationsService, S3VerifierService};
use crate::modules::resolve::ResolveService;
use crate::modules::search::SearchService;
use crate::modules::subscriptions::SubscriptionsService;
use crate::modules::sync_queue::SyncQueueService;
use crate::modules::tracks::TracksService;
use crate::modules::transcode::TranscodeTriggerService;
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

    let pg = db::connect(&config)
        .await
        .expect("Failed to connect to PostgreSQL");
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

    let relay_client = build_call_relay("backend").await;
    let sc = match relay_client.clone() {
        Some(r) => sc.with_relay(r),
        None => sc,
    };
    let external_fetcher = crate::common::external_fetch::ExternalFetcher::new(
        http_client.clone(),
        config.soundcloud.proxy_url.clone(),
        relay_client.clone(),
    );

    let oauth_apps = OAuthAppsService::new(pg.clone(), config.clone());
    if let Err(e) = oauth_apps.migrate_env_app().await {
        warn!(error = %e, "OAuthApps env migration failed");
    }
    match oauth_apps.count_active().await {
        Ok(n) => info!(active = n, "Active OAuth apps"),
        Err(e) => warn!(error = %e, "Failed to count active OAuth apps"),
    }

    let auth_health = crate::modules::auth::AuthHealthService::new(redis_pool.clone());
    let auth = AuthService::new(
        pg.clone(),
        sc.clone(),
        oauth_apps.clone(),
        config.clone(),
        auth_health,
    );
    let link = LinkService::new(pg.clone(), auth.clone());

    let oauth_app_tokens = OAuthAppTokenService::new(pg.clone(), sc.clone(), oauth_apps.clone());
    oauth_app_tokens
        .clone()
        .spawn_refresh_loop(shutdown.clone());
    let tokens = TokenProvider::new(auth.clone(), oauth_app_tokens.clone());

    let cache = CacheService::new(redis_pool.clone());
    let list_cache = ListCacheService::new(redis_pool.clone());
    let events = EventsService::new(pg.clone());
    let subscriptions = SubscriptionsService::new(
        pg.clone(),
        config.subscriptions.snapshot_dir.clone(),
        config.subscriptions.always_premium,
    );
    if let Err(e) = subscriptions.restore_from_snapshot().await {
        warn!(error = %e, "subscriptions restore failed");
    }
    subscriptions.spawn_snapshot_loop(shutdown.clone());
    let auras = AurasService::new(pg.clone(), subscriptions.clone());
    let sync_queue =
        SyncQueueService::new(pg.clone(), sc.clone(), auth.clone(), redis_pool.clone());
    let cold_refresh =
        ColdRefreshService::new(sc.clone(), pg.clone(), cache.clone(), config.cold.clone());
    let me = MeService::new(
        sc.clone(),
        pg.clone(),
        list_cache.clone(),
        sync_queue.clone(),
    );
    let tracks = TracksService::new(
        sc.clone(),
        pg.clone(),
        list_cache.clone(),
        sync_queue.clone(),
        cold_refresh.clone(),
        tokens.clone(),
    );
    let playlists = PlaylistsService::new(
        sc.clone(),
        pg.clone(),
        list_cache.clone(),
        sync_queue.clone(),
        cold_refresh.clone(),
        tokens.clone(),
    );
    let users = UsersService::new(
        sc.clone(),
        pg.clone(),
        list_cache.clone(),
        cold_refresh.clone(),
        tokens.clone(),
    );
    let dislikes = DislikesService::new(pg.clone(), events.clone());
    let resolve = ResolveService::new(sc.clone(), tokens.clone());
    let search = SearchService::new(pg.clone(), cache.clone());
    let history = HistoryService::new(pg.clone());
    let featured = FeaturedService::new(pg.clone(), sc.clone(), auth.clone());
    let s3_verifier =
        S3VerifierService::new(http_client.clone(), config.storage.url.clone(), pg.clone());
    let transcode = TranscodeTriggerService::new(
        http_client.clone(),
        config.clone(),
        nats.clone(),
        s3_verifier.clone(),
    );
    let worker = WorkerClient::new(nats.clone(), cache.clone(), qdrant.clone());
    worker.spawn_done_consumer();
    let lrclib = LrclibService::new(external_fetcher.clone());
    let mxm = MusixmatchService::new(external_fetcher.clone(), config.mxm.api_base.clone());
    let genius = GeniusService::new(external_fetcher.clone(), config.genius.clone());
    let netease = NeteaseService::new(external_fetcher.clone(), config.netease.api_base.clone());
    let lyrics = LyricsService::new(
        pg.clone(),
        nats.clone(),
        qdrant.clone(),
        lrclib,
        mxm,
        genius.clone(),
        netease,
        worker.clone(),
        transcode.clone(),
        s3_verifier.clone(),
        config.lyrics.indexing_concurrency,
    );
    lyrics.spawn_consumers();
    lyrics.spawn_reap_loops(shutdown.clone());

    let collab_vector = CollabVectorService::new(qdrant.clone(), pg.clone());
    let collab_trainer = CollabTrainerService::new(
        pg.clone(),
        nats.clone(),
        qdrant.clone(),
        collab_vector.clone(),
        config.collab.clone(),
    );
    collab_trainer.spawn_bootstrap_and_cron(shutdown.clone());

    let indexing = IndexingService::new(
        pg.clone(),
        nats.clone(),
        qdrant.clone(),
        lyrics.clone(),
        transcode.clone(),
        config.max_track_duration_ms,
    );
    indexing.spawn(shutdown.clone());
    cold_refresh.install_indexing(indexing.clone());

    let duration_resolver = crate::modules::indexing::DurationResolver::new(
        pg.clone(),
        resolve.clone(),
        config.max_track_duration_ms,
    );
    duration_resolver.spawn(shutdown.clone());

    let artist_account_walker = crate::modules::enrich::ArtistAccountWalker::new(
        pg.clone(),
        sc.clone(),
        tokens.clone(),
        indexing.clone(),
    );

    let likes = LikesService::new(
        pg.clone(),
        sync_queue.clone(),
        indexing.clone(),
        events.clone(),
    );

    let mb = MbClient::new(
        external_fetcher.clone(),
        config.enrich.mb_user_agent.clone(),
        config.enrich.mb_rate_limit_ms,
    );
    let ai_resolver = if config.enrich.ai_enabled {
        Some(AiResolverClient::new(
            nats.clone(),
            redis_pool.clone(),
            config.enrich.ai_timeout_ms,
            config.enrich.ai_daily_budget,
        ))
    } else {
        None
    };
    let enrich = EnrichService::new(
        pg.clone(),
        mb.clone(),
        genius.clone(),
        ai_resolver,
        config.enrich.clone(),
    );
    if let Some(kicker) = enrich.spawn(shutdown.clone()) {
        indexing.install_enrich_kicker(kicker);
    }

    let artist_crawl = ArtistCrawlService::new(
        pg.clone(),
        mb,
        genius.clone(),
        sc.clone(),
        tokens.clone(),
        resolve.clone(),
    );

    let ai_matcher = if config.enrich.ai_enabled {
        Some(crate::modules::enrich::ai_matcher::AiMatcherClient::new(
            nats.clone(),
            redis_pool.clone(),
            config.enrich.ai_timeout_ms,
            config.enrich.ai_daily_budget,
        ))
    } else {
        None
    };

    let sc_account_scanner = crate::modules::enrich::sc_account_scan::ScAccountScanner::new(
        pg.clone(),
        sc.clone(),
        tokens.clone(),
        indexing.clone(),
        ai_matcher.clone(),
    );

    let wanted_resolver = crate::modules::enrich::WantedResolverService::new(
        pg.clone(),
        sc.clone(),
        tokens.clone(),
        indexing.clone(),
        sc_account_scanner.clone(),
        ai_matcher.clone(),
        &config.enrich_crawl,
    );
    wanted_resolver.spawn(shutdown.clone());
    let wanted_resolver_state = wanted_resolver.clone();

    let discover = DiscoverService::new(pg.clone(), cache.clone(), subscriptions.clone());
    discover.clone().spawn_refresh_loop(shutdown.clone());

    // Catalog discovery (crawl every artist on Genius/MB) on the work pool.
    crate::modules::discovery::spawn(
        pg.clone(),
        artist_crawl.clone(),
        artist_account_walker.clone(),
        wanted_resolver.clone(),
        &config.discovery,
        shutdown.clone(),
    );

    let track_discovery =
        crate::modules::indexing::TrackDiscoveryService::new(sc.clone(), indexing.clone());
    sc.install_track_observer(track_discovery.clone());

    let recommendations = RecommendationsService::new(
        qdrant.clone(),
        pg.clone(),
        redis_pool.clone(),
        worker.clone(),
        s3_verifier.clone(),
        collab_vector.clone(),
        config.soundwave.clone(),
    );

    let vibe = crate::modules::search::VibeSearchService::new(
        pg.clone(),
        cache.clone(),
        recommendations.clone(),
        worker.clone(),
        qdrant.clone(),
    );

    events.install_deps(
        indexing.clone(),
        dislikes.clone(),
        collab_vector.clone(),
        collab_trainer.clone(),
    );

    crate::modules::recommendations::cron::spawn_cron_loops(
        recommendations.clone(),
        nats.clone(),
        shutdown.clone(),
    );

    let mut tasks = JoinSet::new();

    {
        let token = shutdown.clone();
        let sq = sync_queue.clone();
        tasks.spawn(async move {
            run_periodic(
                "sync_queue.flush",
                token,
                BG_TICK,
                BG_WORK_TIMEOUT,
                move || {
                    let sq = sq.clone();
                    async move { sq.flush().await.map(|_| ()) }
                },
            )
            .await;
        });
    }

    {
        let token = shutdown.clone();
        let auth = auth.clone();
        tasks.spawn(async move {
            run_periodic(
                "auth.cleanup_login_requests",
                token,
                BG_TICK,
                BG_WORK_TIMEOUT,
                move || {
                    let auth = auth.clone();
                    async move { auth.cleanup_expired_login_requests().await }
                },
            )
            .await;
        });
    }

    {
        let token = shutdown.clone();
        let auth = auth.clone();
        tasks.spawn(async move {
            run_periodic(
                "auth.cleanup_link_requests",
                token,
                BG_TICK,
                BG_WORK_TIMEOUT,
                move || {
                    let auth = auth.clone();
                    async move { auth.cleanup_expired_link_requests().await }
                },
            )
            .await;
        });
    }

    let port = config.port;
    let state = AppState {
        config: config.clone(),
        pg,
        cache,
        list_cache,
        auth,
        link,
        oauth_apps,
        events,
        dislikes,
        subscriptions,
        auras,
        me,
        tracks,
        playlists,
        users,
        likes,
        resolve,
        search,
        vibe,
        history,
        featured,
        lyrics,
        collab_vector,
        collab_trainer,
        indexing,
        recommendations,
        enrich,
        artist_crawl: artist_crawl.clone(),
        wanted_resolver: wanted_resolver_state,
        discover,
        sync_queue: sync_queue.clone(),
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

async fn build_call_relay(role: &str) -> Option<std::sync::Arc<call_relay::Client>> {
    let endpoint = std::env::var("CALL_CONTROL_ENDPOINT").ok()?;
    if endpoint.is_empty() {
        return None;
    }
    let relay_secret = std::env::var("CALL_RELAY_SECRET").unwrap_or_default();
    if relay_secret.is_empty() {
        tracing::warn!(
            role,
            "CALL_RELAY_SECRET empty; relay will be rejected by server"
        );
    }
    let cfg = call_relay::Config {
        control_endpoint: Some(endpoint),
        upstream_proxy: None,
        instance_id: format!("{role}-{}", std::process::id()),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        relay_secret,
        policy: call_relay::tiers::Policy {
            // Только client-тир — direct/proxy выполняет вызывающая сторона.
            order: vec![call_relay::Tier::Client],
            timeout_ms: 15_000,
            fallback_on_status_5xx: true,
        },
    };
    match call_relay::Client::connect(cfg).await {
        Ok(c) => {
            tracing::info!(role, "call-relay connected");
            Some(std::sync::Arc::new(c))
        }
        Err(e) => {
            tracing::warn!(role, error = %e, "call-relay connect failed; running without it");
            None
        }
    }
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
