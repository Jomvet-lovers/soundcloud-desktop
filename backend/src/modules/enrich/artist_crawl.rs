use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::config::EnrichCrawlCfg;
use crate::error::AppResult;
use crate::modules::auth::{try_with_chain, TokenKind, TokenProvider};
use crate::modules::enrich::mb::{MbArtistUrl, MbClient, MbRecordingBrief};
use crate::modules::enrich::normalize::{normalize_name, normalize_title};
use crate::modules::enrich::sc_accounts::{
    self, extract_sc_user_id_from_resolve, is_soundcloud_url, AccountRole,
};
use crate::modules::lyrics::genius::{
    GeniusAlbumTrack, GeniusArtistDetails, GeniusService, GeniusSongMeta,
};
use crate::modules::resolve::ResolveService;
use crate::sc::ScClient;

const MB_PAGE_SIZE: u32 = 100;
const GENIUS_PAGE_SIZE: u32 = 50;

pub struct ArtistCrawlService {
    pg: PgPool,
    mb: Arc<MbClient>,
    genius: Arc<GeniusService>,
    sc: ScClient,
    tokens: Arc<TokenProvider>,
    resolve: Arc<ResolveService>,
    cfg: EnrichCrawlCfg,
}

type ArtistRow = (
    Uuid,
    Option<String>,
    Option<String>,
    Option<String>,
    i32,
    i32,
);

#[derive(Debug, Clone, Copy)]
enum CrawlSource {
    Mb,
    Genius,
}

impl ArtistCrawlService {
    pub fn new(
        pg: PgPool,
        mb: Arc<MbClient>,
        genius: Arc<GeniusService>,
        sc: ScClient,
        tokens: Arc<TokenProvider>,
        resolve: Arc<ResolveService>,
        cfg: EnrichCrawlCfg,
    ) -> Arc<Self> {
        Arc::new(Self {
            pg,
            mb,
            genius,
            sc,
            tokens,
            resolve,
            cfg,
        })
    }

    pub fn spawn(self: &Arc<Self>, shutdown: CancellationToken) {
        if !self.cfg.enabled {
            info!("artist crawl disabled by config");
            return;
        }
        self.spawn_fresh_pickup(shutdown.clone());
        let svc = self.clone();
        tokio::spawn(async move {
            let interval = Duration::from_secs(svc.cfg.interval_sec.max(60));
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticker.tick().await;
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    _ = ticker.tick() => {
                        if let Err(e) = svc.run_tick().await {
                            warn!(error = %e, "artist crawl tick failed");
                        }
                    }
                }
            }
        });
    }

    async fn run_tick(&self) -> AppResult<()> {
        let stale_after = chrono::Duration::hours(self.cfg.stale_after_hours.max(1) as i64);
        let stale_cutoff = chrono::Utc::now() - stale_after;
        let max_attempts = self.cfg.max_attempts as i16;
        let batch = self.cfg.batch_size.max(1);

        let rows: Vec<ArtistRow> = sqlx::query_as(
            "WITH activity AS (
                 SELECT ta.artist_id, COUNT(*)::int8 AS event_count
                 FROM user_events ue
                 JOIN tracks it ON it.sc_track_id = ue.sc_track_id
                 JOIN track_artists ta ON ta.track_id = it.id
                 WHERE ue.created_at > now() - interval '30 days'
                 GROUP BY ta.artist_id
             ),
             picked AS (
                 SELECT a.id
                 FROM artists a
                 LEFT JOIN activity ac ON ac.artist_id = a.id
                 WHERE a.merged_into IS NULL
                   AND a.confidence >= 0.5
                   AND a.crawl_attempts < $1
                   AND (a.last_crawled_at IS NULL OR a.last_crawled_at < $2)
                 ORDER BY ac.event_count DESC NULLS LAST,
                          a.last_crawled_at NULLS FIRST,
                          a.confidence DESC
                 LIMIT $3
                 FOR UPDATE SKIP LOCKED
             )
             UPDATE artists a
             SET last_crawled_at = now(),
                 crawl_attempts = a.crawl_attempts + 1
             FROM picked
             WHERE a.id = picked.id
             RETURNING a.id, a.mb_artist_id, a.genius_artist_id, a.sc_user_id, a.mb_crawl_offset, a.genius_crawl_offset",
        )
        .bind(max_attempts)
        .bind(stale_cutoff)
        .bind(batch)
        .fetch_all(&self.pg)
        .await?;

        if rows.is_empty() {
            return Ok(());
        }
        info!(count = rows.len(), "artist crawl: processing batch");

        for (artist_id, mb_id, genius_id, sc_user_id, mb_off, genius_off) in rows {
            if let Err(e) = self
                .crawl_one(
                    artist_id,
                    mb_id.as_deref(),
                    genius_id.as_deref(),
                    sc_user_id.as_deref(),
                    mb_off as u32,
                    genius_off as u32,
                )
                .await
            {
                warn!(artist = %artist_id, error = %e, "artist crawl failed");
            }
        }
        Ok(())
    }

    fn spawn_fresh_pickup(self: &Arc<Self>, shutdown: CancellationToken) {
        let svc = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(60));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticker.tick().await;
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    _ = ticker.tick() => {
                        if let Err(e) = svc.pickup_fresh().await {
                            warn!(error = %e, "fresh-pickup failed");
                        }
                    }
                }
            }
        });
    }

    async fn pickup_fresh(self: &Arc<Self>) -> AppResult<()> {
        let rows: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM artists
             WHERE merged_into IS NULL
               AND last_crawled_at IS NULL
               AND confidence >= 0.5
               AND crawl_attempts < $1
               AND (mb_artist_id IS NOT NULL OR genius_artist_id IS NOT NULL)
             ORDER BY confidence DESC, created_at
             LIMIT 5",
        )
        .bind(self.cfg.max_attempts as i16)
        .fetch_all(&self.pg)
        .await?;
        if rows.is_empty() {
            return Ok(());
        }
        info!(count = rows.len(), "artist crawl: fresh-pickup batch");
        for (artist_id,) in rows {
            let svc = self.clone();
            tokio::spawn(async move {
                if let Err(e) = svc.run_for_artist(artist_id).await {
                    warn!(%artist_id, error = %e, "fresh-pickup run_for_artist failed");
                }
            });
        }
        Ok(())
    }

    pub async fn run_for_artist(self: &Arc<Self>, artist_id: Uuid) -> AppResult<()> {
        let claim: Option<ArtistRow> = sqlx::query_as(
            "UPDATE artists
             SET last_crawled_at = now(), crawl_attempts = crawl_attempts + 1
             WHERE id = $1
               AND merged_into IS NULL
               AND (last_crawled_at IS NULL OR last_crawled_at < now() - interval '5 minutes')
             RETURNING id, mb_artist_id, genius_artist_id, sc_user_id, mb_crawl_offset, genius_crawl_offset",
        )
        .bind(artist_id)
        .fetch_optional(&self.pg)
        .await?;
        let Some((id, mb_id, genius_id, sc_user_id, mb_off, genius_off)) = claim else {
            return Ok(());
        };
        self.crawl_one(
            id,
            mb_id.as_deref(),
            genius_id.as_deref(),
            sc_user_id.as_deref(),
            mb_off as u32,
            genius_off as u32,
        )
        .await
    }

    async fn crawl_one(
        &self,
        artist_id: Uuid,
        mb_id: Option<&str>,
        genius_id: Option<&str>,
        sc_user_id: Option<&str>,
        mb_offset: u32,
        genius_offset: u32,
    ) -> AppResult<()> {
        let mut socials: Vec<(String, String, String)> = Vec::new();
        let mut country: Option<String> = None;
        let mut avatar_url: Option<String> = None;
        let mut bio: Option<String> = None;

        if let Some(mb_id) = mb_id {
            match self.mb.lookup_artist(mb_id).await {
                Ok(Some(details)) => {
                    if let Some(c) = details.country.as_deref() {
                        if !c.is_empty() {
                            country = Some(c.to_string());
                        }
                    }
                    if let Some(d) = details.disambiguation {
                        if !d.is_empty() {
                            bio = Some(d);
                        }
                    }
                    for u in details.urls {
                        for entry in normalize_mb_url(&u) {
                            socials.push(entry);
                        }
                    }
                }
                Ok(None) => debug!(artist = %artist_id, mb_id, "MB artist 404"),
                Err(e) => debug!(error = %e, "MB lookup_artist failed"),
            }
        }

        if let Some(genius_id) = genius_id.and_then(|s| s.parse::<i64>().ok()) {
            if let Some(details) = self.genius.lookup_artist(genius_id).await {
                if let Some(av) = details.avatar_url.clone() {
                    if !av.is_empty() {
                        avatar_url = Some(av);
                    }
                }
                socials.extend(genius_socials(&details, "genius"));
            }
        }

        socials.sort_by(|a, b| a.1.cmp(&b.1));
        socials.dedup_by(|a, b| a.1 == b.1);

        if !socials.is_empty() {
            self.upsert_socials(artist_id, &socials).await?;
        }
        self.maybe_update_metadata(
            artist_id,
            country.as_deref(),
            avatar_url.as_deref(),
            bio.as_deref(),
        )
        .await?;

        if let Err(e) = self.resolve_sc_accounts(artist_id, &socials).await {
            debug!(artist = %artist_id, error = %e, "SC accounts resolve failed");
        }

        if let Some(mb_id) = mb_id {
            match self.discover_mb_tracks(artist_id, mb_id, mb_offset).await {
                Ok(next) => {
                    self.set_crawl_offset(artist_id, CrawlSource::Mb, next)
                        .await?
                }
                Err(e) => debug!(artist = %artist_id, error = %e, "MB track discovery failed"),
            }
        }
        if let Some(gid) = genius_id.and_then(|s| s.parse::<i64>().ok()) {
            match self
                .discover_genius_songs(artist_id, gid, genius_offset)
                .await
            {
                Ok(next) => {
                    self.set_crawl_offset(artist_id, CrawlSource::Genius, next)
                        .await?
                }
                Err(e) => debug!(artist = %artist_id, error = %e, "Genius song discovery failed"),
            }
            if let Err(e) = self.discover_genius_albums(artist_id, gid).await {
                debug!(artist = %artist_id, error = %e, "Genius album discovery failed");
            }
        }
        if let Some(sc_user_id) = sc_user_id {
            if let Err(e) = self.fetch_sc_web_profiles(artist_id, sc_user_id).await {
                debug!(artist = %artist_id, error = %e, "SC web-profiles fetch failed");
            }
        }
        Ok(())
    }

    async fn resolve_sc_accounts(
        &self,
        artist_id: Uuid,
        socials: &[(String, String, String)],
    ) -> AppResult<()> {
        for (kind, url, _source) in socials {
            if kind != "soundcloud" || !is_soundcloud_url(url) {
                continue;
            }
            let value = match self.resolve.resolve(TokenKind::PublicPool, url).await {
                Ok(v) => v,
                Err(e) => {
                    debug!(url, error = %e, "resolve sc url failed");
                    continue;
                }
            };
            let Some(sc_user_id) = extract_sc_user_id_from_resolve(&value) else {
                continue;
            };
            sc_accounts::upsert(
                &self.pg,
                artist_id,
                &sc_user_id,
                AccountRole::Main,
                "mb_resolve",
                false,
            )
            .await?;
        }
        Ok(())
    }

    async fn set_crawl_offset(
        &self,
        artist_id: Uuid,
        source: CrawlSource,
        offset: u32,
    ) -> AppResult<()> {
        let sql = match source {
            CrawlSource::Mb => "UPDATE artists SET mb_crawl_offset = $2 WHERE id = $1",
            CrawlSource::Genius => "UPDATE artists SET genius_crawl_offset = $2 WHERE id = $1",
        };
        sqlx::query(sql)
            .bind(artist_id)
            .bind(offset as i32)
            .execute(&self.pg)
            .await?;
        Ok(())
    }

    async fn discover_mb_tracks(
        &self,
        artist_id: Uuid,
        mb_id: &str,
        starting_offset: u32,
    ) -> AppResult<u32> {
        let mut offset = starting_offset;
        for _ in 0..10 {
            let recordings = self
                .mb
                .browse_recordings_by_artist(mb_id, offset, MB_PAGE_SIZE)
                .await?;
            let count = recordings.len() as u32;
            for rec in recordings {
                if let Err(e) = self.persist_mb_recording(artist_id, mb_id, rec).await {
                    debug!(error = %e, "wanted_track upsert (mb) failed");
                }
            }
            if count < MB_PAGE_SIZE {
                return Ok(0);
            }
            offset += count;
        }
        Ok(offset)
    }

    async fn discover_genius_albums(&self, artist_id: Uuid, genius_id: i64) -> AppResult<()> {
        let mut page = 1u32;
        for _ in 0..10 {
            let (albums, has_more) = self.genius.list_artist_albums(genius_id, page, 20).await;
            if albums.is_empty() {
                break;
            }
            for album_ref in albums {
                let year_hint = album_ref.year;
                let genius_album_id = album_ref.genius_album_id;
                let album_id = self
                    .ensure_genius_album(album_ref, Some(artist_id), year_hint)
                    .await?;
                let Some(album_id) = album_id else { continue };
                if let Err(e) = self
                    .ingest_genius_album_tracks(artist_id, album_id, genius_album_id)
                    .await
                {
                    debug!(album_id = %album_id, error = %e, "Genius album tracks ingest failed");
                }
            }
            if !has_more {
                return Ok(());
            }
            page += 1;
        }
        Ok(())
    }

    pub async fn ingest_genius_album_tracks(
        &self,
        primary_artist_id: Uuid,
        album_id: Uuid,
        genius_album_id: i64,
    ) -> AppResult<()> {
        let mut page = 1u32;
        for _ in 0..6 {
            let (tracks, has_more) = self
                .genius
                .list_album_tracks(genius_album_id, page, 50)
                .await;
            if tracks.is_empty() {
                break;
            }
            for track in tracks {
                if let Err(e) = self
                    .upsert_album_track(primary_artist_id, album_id, track)
                    .await
                {
                    debug!(error = %e, "album track upsert failed");
                }
            }
            if !has_more {
                return Ok(());
            }
            page += 1;
        }
        Ok(())
    }

    async fn upsert_album_track(
        &self,
        primary_artist_id: Uuid,
        album_id: Uuid,
        track: GeniusAlbumTrack,
    ) -> AppResult<()> {
        let normalized = normalize_title(&track.title);
        if normalized.is_empty() {
            return Ok(());
        }
        let track_primary_id = match track.primary_artist.as_ref() {
            Some(p) => self
                .ensure_external_artist(
                    None,
                    p.genius_artist_id.map(|i| i.to_string()).as_deref(),
                    &p.name,
                )
                .await?
                .or(Some(primary_artist_id)),
            None => Some(primary_artist_id),
        };
        let position = track
            .position
            .and_then(|n| i16::try_from(n).ok())
            .unwrap_or(0);

        if let Some(pa_id) = track_primary_id {
            if let Some(indexed_id) = self
                .indexed_track_for_artist_title(pa_id, &track.title)
                .await?
            {
                self.link_indexed_album_with_position(indexed_id, album_id, position)
                    .await?;
                return Ok(());
            }
        }

        let external_id = track.genius_song_id.to_string();
        let wanted_id: Option<(Uuid,)> = sqlx::query_as(
            "INSERT INTO wanted_tracks
                (title, normalized_title, primary_artist_id, source, external_id)
             VALUES ($1, $2, $3, 'genius_crawl', $4)
             ON CONFLICT (source, external_id) WHERE external_id IS NOT NULL DO UPDATE
                SET primary_artist_id = COALESCE(wanted_tracks.primary_artist_id, EXCLUDED.primary_artist_id),
                    updated_at        = now()
             RETURNING id",
        )
        .bind(track.title.trim())
        .bind(&normalized)
        .bind(track_primary_id)
        .bind(&external_id)
        .fetch_optional(&self.pg)
        .await?;
        let Some((wanted_id,)) = wanted_id else {
            return Ok(());
        };
        if let Some(pa) = track_primary_id {
            self.insert_wanted_artist(wanted_id, pa, "primary", 0)
                .await?;
        }
        for (pos, fa) in track.featured.iter().enumerate() {
            let id = self
                .ensure_external_artist(
                    None,
                    fa.genius_artist_id.map(|i| i.to_string()).as_deref(),
                    &fa.name,
                )
                .await?;
            if let Some(id) = id {
                self.insert_wanted_artist(wanted_id, id, "featured", pos as i16)
                    .await?;
            }
        }
        self.link_wanted_album(wanted_id, album_id, position)
            .await?;
        Ok(())
    }

    async fn link_indexed_album_with_position(
        &self,
        track_id: Uuid,
        album_id: Uuid,
        position: i16,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE tracks SET album_id = COALESCE(album_id, $2), album_position = COALESCE(album_position, $3) WHERE id = $1",
        )
        .bind(track_id)
        .bind(album_id)
        .bind(position)
        .execute(&self.pg)
        .await?;
        sqlx::query(
            "INSERT INTO album_tracks (album_id, track_id, position) VALUES ($1, $2, $3)
             ON CONFLICT DO NOTHING",
        )
        .bind(album_id)
        .bind(track_id)
        .bind(position)
        .execute(&self.pg)
        .await?;
        Ok(())
    }

    async fn discover_genius_songs(
        &self,
        artist_id: Uuid,
        genius_id: i64,
        starting_offset: u32,
    ) -> AppResult<u32> {
        let mut offset = starting_offset;
        for _ in 0..10 {
            let page = (offset / GENIUS_PAGE_SIZE) + 1;
            let songs = self
                .genius
                .list_artist_songs(genius_id, page, GENIUS_PAGE_SIZE)
                .await;
            let count = songs.len() as u32;
            for song in songs {
                if let Err(e) = self.persist_genius_song(artist_id, genius_id, song).await {
                    debug!(error = %e, "wanted_track upsert (genius) failed");
                }
            }
            if count < GENIUS_PAGE_SIZE {
                return Ok(0);
            }
            offset += count;
        }
        Ok(offset)
    }

    async fn persist_mb_recording(
        &self,
        crawled_artist_id: Uuid,
        crawled_mb_id: &str,
        rec: MbRecordingBrief,
    ) -> AppResult<()> {
        let primary_artist_id = match rec.primary_artist.as_ref() {
            Some(a) if a.mb_id == crawled_mb_id => Some(crawled_artist_id),
            Some(a) => {
                self.ensure_external_artist(Some(&a.mb_id), None, &a.name)
                    .await?
            }
            None => None,
        };

        if let Some(isrc) = rec.isrc.as_deref() {
            if self.indexed_track_has_isrc(isrc).await? {
                return Ok(());
            }
        }

        let normalized_title = normalize_title(&rec.title);
        if normalized_title.is_empty() {
            return Ok(());
        }

        let album_id = if let Some(rel) = rec.release.as_ref() {
            self.ensure_external_album(rel, primary_artist_id).await?
        } else {
            None
        };

        let wanted_id: Option<(Uuid,)> = sqlx::query_as(
            "INSERT INTO wanted_tracks
                (title, normalized_title, primary_artist_id, isrc, duration_ms, release_year, source, external_id)
             VALUES ($1, $2, $3, $4, $5, $6, 'mb_crawl', $7)
             ON CONFLICT (source, external_id) WHERE external_id IS NOT NULL DO UPDATE
                SET primary_artist_id = COALESCE(wanted_tracks.primary_artist_id, EXCLUDED.primary_artist_id),
                    isrc              = COALESCE(wanted_tracks.isrc, EXCLUDED.isrc),
                    duration_ms       = COALESCE(wanted_tracks.duration_ms, EXCLUDED.duration_ms),
                    release_year      = COALESCE(wanted_tracks.release_year, EXCLUDED.release_year),
                    updated_at        = now()
             RETURNING id",
        )
        .bind(rec.title.trim())
        .bind(&normalized_title)
        .bind(primary_artist_id)
        .bind(rec.isrc.as_deref())
        .bind(rec.length_ms)
        .bind(rec.first_release_year)
        .bind(&rec.mb_id)
        .fetch_optional(&self.pg)
        .await?;

        let Some((wanted_id,)) = wanted_id else {
            return Ok(());
        };
        if let Some(album_id) = album_id {
            self.link_wanted_album(wanted_id, album_id, 0).await?;
        }
        if let Some(pa) = primary_artist_id {
            self.insert_wanted_artist(wanted_id, pa, "primary", 0)
                .await?;
        }
        for (pos, fa) in rec.featured.iter().enumerate() {
            let id = self
                .ensure_external_artist(Some(&fa.mb_id), None, &fa.name)
                .await?;
            if let Some(id) = id {
                self.insert_wanted_artist(wanted_id, id, "featured", pos as i16)
                    .await?;
            }
        }
        Ok(())
    }

    async fn ensure_external_album(
        &self,
        rel: &crate::modules::enrich::mb::MbReleaseBrief,
        primary_artist_id: Option<Uuid>,
    ) -> AppResult<Option<Uuid>> {
        let row: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM albums WHERE mb_release_id = $1 LIMIT 1")
                .bind(&rel.mb_id)
                .fetch_optional(&self.pg)
                .await?;
        if let Some((id,)) = row {
            return Ok(Some(id));
        }
        let normalized_title = normalize_title(&rel.title);
        if normalized_title.is_empty() {
            return Ok(None);
        }
        let kind = match rel.release_type.as_deref() {
            Some("EP") => "ep",
            Some("Single") => "single",
            Some("Compilation") => "compilation",
            _ => "album",
        };
        let inserted: Option<(Uuid,)> = sqlx::query_as(
            "INSERT INTO albums (title, normalized_title, primary_artist_id, type, release_year, mb_release_id, source, confidence)
             VALUES ($1, $2, $3, $4, $5, $6, 'mb_crawl', 0.7)
             ON CONFLICT DO NOTHING
             RETURNING id",
        )
        .bind(rel.title.trim())
        .bind(&normalized_title)
        .bind(primary_artist_id)
        .bind(kind)
        .bind(rel.year)
        .bind(&rel.mb_id)
        .fetch_optional(&self.pg)
        .await?;
        if let Some((id,)) = inserted {
            if let Some(pa) = primary_artist_id {
                let _ = sqlx::query(
                    "INSERT INTO album_artists (album_id, artist_id, role)
                     VALUES ($1, $2, 'primary')
                     ON CONFLICT DO NOTHING",
                )
                .bind(id)
                .bind(pa)
                .execute(&self.pg)
                .await?;
            }
            return Ok(Some(id));
        }
        let again: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM albums WHERE mb_release_id = $1 LIMIT 1")
                .bind(&rel.mb_id)
                .fetch_optional(&self.pg)
                .await?;
        Ok(again.map(|(id,)| id))
    }

    async fn insert_wanted_artist(
        &self,
        wanted_track_id: Uuid,
        artist_id: Uuid,
        role: &str,
        position: i16,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO wanted_track_artists (wanted_track_id, artist_id, role, position)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT DO NOTHING",
        )
        .bind(wanted_track_id)
        .bind(artist_id)
        .bind(role)
        .bind(position)
        .execute(&self.pg)
        .await?;
        Ok(())
    }

    async fn link_wanted_album(
        &self,
        wanted_track_id: Uuid,
        album_id: Uuid,
        position: i16,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO wanted_track_albums (wanted_track_id, album_id, position)
             VALUES ($1, $2, $3)
             ON CONFLICT DO NOTHING",
        )
        .bind(wanted_track_id)
        .bind(album_id)
        .bind(position)
        .execute(&self.pg)
        .await?;
        Ok(())
    }

    async fn persist_genius_song(
        &self,
        crawled_artist_id: Uuid,
        crawled_genius_id: i64,
        song: GeniusSongMeta,
    ) -> AppResult<()> {
        let Some(primary) = song.primary_artist else {
            return Ok(());
        };
        let primary_artist_id = match primary.genius_artist_id {
            Some(gid) if gid == crawled_genius_id => Some(crawled_artist_id),
            _ => {
                self.ensure_external_artist(
                    None,
                    primary.genius_artist_id.map(|i| i.to_string()).as_deref(),
                    &primary.name,
                )
                .await?
            }
        };

        let normalized_title = normalize_title(&song.title);
        if normalized_title.is_empty() {
            return Ok(());
        }
        let already_indexed_id = if let Some(pa_id) = primary_artist_id {
            self.indexed_track_for_artist_title(pa_id, &song.title)
                .await?
        } else {
            None
        };
        if let Some(indexed_id) = already_indexed_id {
            if let Some(genius_song_id) = song.genius_song_id {
                if let Some(details) = self.genius.lookup_song(genius_song_id).await {
                    if let Some(album_ref) = details.album {
                        let album_id = self
                            .ensure_genius_album(album_ref, primary_artist_id, details.year)
                            .await?;
                        if let Some(album_id) = album_id {
                            self.link_indexed_album(indexed_id, album_id).await?;
                        }
                    }
                }
            }
            return Ok(());
        }
        let external_id = match song.genius_song_id {
            Some(id) => id.to_string(),
            None => return Ok(()),
        };
        let wanted_id: Option<(Uuid,)> = sqlx::query_as(
            "INSERT INTO wanted_tracks
                (title, normalized_title, primary_artist_id, source, external_id)
             VALUES ($1, $2, $3, 'genius_crawl', $4)
             ON CONFLICT (source, external_id) WHERE external_id IS NOT NULL DO UPDATE
                SET primary_artist_id = COALESCE(wanted_tracks.primary_artist_id, EXCLUDED.primary_artist_id),
                    updated_at        = now()
             RETURNING id",
        )
        .bind(song.title.trim())
        .bind(&normalized_title)
        .bind(primary_artist_id)
        .bind(&external_id)
        .fetch_optional(&self.pg)
        .await?;

        let Some((wanted_id,)) = wanted_id else {
            return Ok(());
        };
        if let Some(pa) = primary_artist_id {
            self.insert_wanted_artist(wanted_id, pa, "primary", 0)
                .await?;
        }
        for (pos, fa) in song.featured.iter().enumerate() {
            let id = self
                .ensure_external_artist(
                    None,
                    fa.genius_artist_id.map(|i| i.to_string()).as_deref(),
                    &fa.name,
                )
                .await?;
            if let Some(id) = id {
                self.insert_wanted_artist(wanted_id, id, "featured", pos as i16)
                    .await?;
            }
        }
        if let Some(genius_song_id) = song.genius_song_id {
            if let Some(details) = self.genius.lookup_song(genius_song_id).await {
                if let Some(album_ref) = details.album {
                    let album_id = self
                        .ensure_genius_album(album_ref, primary_artist_id, details.year)
                        .await?;
                    if let Some(album_id) = album_id {
                        self.link_wanted_album(wanted_id, album_id, 0).await?;
                    }
                }
            }
        }
        Ok(())
    }

    async fn ensure_genius_album(
        &self,
        album_ref: crate::modules::lyrics::genius::GeniusAlbumRef,
        primary_artist_id: Option<Uuid>,
        song_year: Option<i16>,
    ) -> AppResult<Option<Uuid>> {
        let genius_id_str = album_ref.genius_album_id.to_string();
        let row: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM albums WHERE genius_album_id = $1 LIMIT 1")
                .bind(&genius_id_str)
                .fetch_optional(&self.pg)
                .await?;
        if let Some((id,)) = row {
            sqlx::query(
                "UPDATE albums SET cover_url = COALESCE(cover_url, $2),
                                   release_year = COALESCE(release_year, $3),
                                   updated_at = now()
                 WHERE id = $1",
            )
            .bind(id)
            .bind(album_ref.cover_url.as_deref())
            .bind(album_ref.year.or(song_year))
            .execute(&self.pg)
            .await?;
            if let Some(pa_id) = primary_artist_id {
                let _ = sqlx::query(
                    "INSERT INTO album_artists (album_id, artist_id, role)
                     VALUES ($1, $2, 'primary')
                     ON CONFLICT DO NOTHING",
                )
                .bind(id)
                .bind(pa_id)
                .execute(&self.pg)
                .await?;
            }
            return Ok(Some(id));
        }
        let normalized = normalize_title(&album_ref.name);
        if normalized.is_empty() {
            return Ok(None);
        }
        let inserted: Option<(Uuid,)> = sqlx::query_as(
            "INSERT INTO albums (title, normalized_title, primary_artist_id, type, release_year, genius_album_id, cover_url, source, confidence)
             VALUES ($1, $2, $3, 'album', $4, $5, $6, 'genius_crawl', 0.7)
             ON CONFLICT DO NOTHING
             RETURNING id",
        )
        .bind(album_ref.name.trim())
        .bind(&normalized)
        .bind(primary_artist_id)
        .bind(album_ref.year.or(song_year))
        .bind(&genius_id_str)
        .bind(album_ref.cover_url.as_deref())
        .fetch_optional(&self.pg)
        .await?;
        if let Some((id,)) = inserted {
            if let Some(pa_id) = primary_artist_id {
                let _ = sqlx::query(
                    "INSERT INTO album_artists (album_id, artist_id, role)
                     VALUES ($1, $2, 'primary')
                     ON CONFLICT DO NOTHING",
                )
                .bind(id)
                .bind(pa_id)
                .execute(&self.pg)
                .await?;
            }
            return Ok(Some(id));
        }
        let again: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM albums WHERE genius_album_id = $1 LIMIT 1")
                .bind(&genius_id_str)
                .fetch_optional(&self.pg)
                .await?;
        Ok(again.map(|(id,)| id))
    }

    async fn ensure_external_artist(
        &self,
        mb_id: Option<&str>,
        genius_id: Option<&str>,
        name: &str,
    ) -> AppResult<Option<Uuid>> {
        if let Some(mb) = mb_id {
            let row: Option<(Uuid,)> =
                sqlx::query_as("SELECT id FROM artists WHERE mb_artist_id = $1 LIMIT 1")
                    .bind(mb)
                    .fetch_optional(&self.pg)
                    .await?;
            if let Some((id,)) = row {
                return Ok(Some(id));
            }
        }
        if let Some(gid) = genius_id {
            let row: Option<(Uuid,)> =
                sqlx::query_as("SELECT id FROM artists WHERE genius_artist_id = $1 LIMIT 1")
                    .bind(gid)
                    .fetch_optional(&self.pg)
                    .await?;
            if let Some((id,)) = row {
                return Ok(Some(id));
            }
        }
        let cleaned = crate::modules::enrich::normalize::clean_artist_name(name);
        if cleaned.is_empty() {
            return Ok(None);
        }
        let normalized = normalize_name(&cleaned);
        if normalized.is_empty() {
            return Ok(None);
        }
        let row: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM artists WHERE normalized_name = $1 AND merged_into IS NULL LIMIT 1",
        )
        .bind(&normalized)
        .fetch_optional(&self.pg)
        .await?;
        if let Some((id,)) = row {
            if mb_id.is_some() || genius_id.is_some() {
                sqlx::query(
                    "UPDATE artists
                     SET mb_artist_id     = COALESCE(mb_artist_id, $2),
                         genius_artist_id = COALESCE(genius_artist_id, $3),
                         updated_at = now()
                     WHERE id = $1",
                )
                .bind(id)
                .bind(mb_id)
                .bind(genius_id)
                .execute(&self.pg)
                .await?;
            }
            return Ok(Some(id));
        }
        let inserted: Option<(Uuid,)> = sqlx::query_as(
            "INSERT INTO artists (name, normalized_name, mb_artist_id, genius_artist_id, source, confidence)
             VALUES ($1, $2, $3, $4, 'crawl', 0.7)
             ON CONFLICT DO NOTHING
             RETURNING id",
        )
        .bind(&cleaned)
        .bind(&normalized)
        .bind(mb_id)
        .bind(genius_id)
        .fetch_optional(&self.pg)
        .await?;
        if let Some((id,)) = inserted {
            return Ok(Some(id));
        }
        let again: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM artists WHERE normalized_name = $1 AND merged_into IS NULL LIMIT 1",
        )
        .bind(&normalized)
        .fetch_optional(&self.pg)
        .await?;
        Ok(again.map(|(id,)| id))
    }

    async fn indexed_track_has_isrc(&self, isrc: &str) -> AppResult<bool> {
        let row: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM tracks WHERE isrc = $1 LIMIT 1")
            .bind(isrc)
            .fetch_optional(&self.pg)
            .await?;
        Ok(row.is_some())
    }

    async fn indexed_track_for_artist_title(
        &self,
        artist_id: Uuid,
        target_title: &str,
    ) -> AppResult<Option<Uuid>> {
        Ok(
            crate::modules::enrich::wanted_resolver::find_best_indexed_for_artist_title(
                &self.pg,
                artist_id,
                target_title,
            )
            .await?
            .map(|m| m.track_id),
        )
    }

    async fn link_indexed_album(&self, track_id: Uuid, album_id: Uuid) -> AppResult<()> {
        sqlx::query("UPDATE tracks SET album_id = COALESCE(album_id, $2) WHERE id = $1")
            .bind(track_id)
            .bind(album_id)
            .execute(&self.pg)
            .await?;
        sqlx::query(
            "INSERT INTO album_tracks (album_id, track_id) VALUES ($1, $2)
             ON CONFLICT DO NOTHING",
        )
        .bind(album_id)
        .bind(track_id)
        .execute(&self.pg)
        .await?;
        Ok(())
    }

    async fn fetch_sc_web_profiles(&self, artist_id: Uuid, sc_user_id: &str) -> AppResult<()> {
        let chain = match self.tokens.chain(TokenKind::PublicPool).await {
            Ok(c) => c,
            Err(_) => return Ok(()),
        };
        let path = format!("/users/soundcloud:users:{sc_user_id}/web-profiles");
        match try_with_chain(&chain, |t| {
            let sc = self.sc.clone();
            let path = path.clone();
            async move { sc.api_get_value(&path, &t, None).await }
        })
        .await
        {
            Ok(value) => {
                let socials = parse_sc_web_profiles(&value);
                if !socials.is_empty() {
                    self.upsert_socials(artist_id, &socials).await?;
                }
            }
            Err(e) => debug!(artist = %artist_id, error = %e, "SC web-profiles failed"),
        }
        Ok(())
    }

    async fn upsert_socials(
        &self,
        artist_id: Uuid,
        rows: &[(String, String, String)],
    ) -> AppResult<()> {
        for (kind, url, source) in rows {
            sqlx::query(
                "INSERT INTO artist_socials (artist_id, kind, url, source)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (artist_id, url) DO UPDATE
                   SET kind = EXCLUDED.kind,
                       source = EXCLUDED.source,
                       updated_at = now()",
            )
            .bind(artist_id)
            .bind(kind)
            .bind(url)
            .bind(source)
            .execute(&self.pg)
            .await?;
        }
        Ok(())
    }

    async fn maybe_update_metadata(
        &self,
        artist_id: Uuid,
        country: Option<&str>,
        avatar_url: Option<&str>,
        bio: Option<&str>,
    ) -> AppResult<()> {
        if country.is_none() && avatar_url.is_none() && bio.is_none() {
            return Ok(());
        }
        sqlx::query(
            "UPDATE artists
             SET country    = COALESCE(country,    $2),
                 avatar_url = COALESCE(avatar_url, $3),
                 bio        = COALESCE(bio,        $4),
                 updated_at = now()
             WHERE id = $1",
        )
        .bind(artist_id)
        .bind(country)
        .bind(avatar_url)
        .bind(bio)
        .execute(&self.pg)
        .await?;
        Ok(())
    }
}

fn normalize_mb_url(u: &MbArtistUrl) -> Vec<(String, String, String)> {
    let url = u.url.trim();
    if url.is_empty() {
        return Vec::new();
    }
    let kind = classify_url(url).unwrap_or_else(|| u.kind.clone());
    vec![(kind, url.to_string(), "mb".to_string())]
}

fn classify_url(raw: &str) -> Option<String> {
    let parsed = url::Url::parse(raw).ok()?;
    let host = parsed.host_str()?.to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(host.as_str());
    let kind = match host {
        "instagram.com" => "instagram",
        "twitter.com" | "x.com" | "mobile.twitter.com" => "twitter",
        "facebook.com" | "m.facebook.com" | "fb.com" | "fb.me" => "facebook",
        "youtube.com" | "youtu.be" | "music.youtube.com" => "youtube",
        "soundcloud.com" | "m.soundcloud.com" => "soundcloud",
        "spotify.com" | "open.spotify.com" => "spotify",
        "music.apple.com" | "itunes.apple.com" => "apple_music",
        "bandcamp.com" => "bandcamp",
        "tiktok.com" | "vm.tiktok.com" => "tiktok",
        "discogs.com" => "discogs",
        "last.fm" | "lastfm.com" | "lastfm.de" => "lastfm",
        "genius.com" => "genius",
        "musicbrainz.org" => "musicbrainz",
        "vk.com" => "vk",
        "telegram.me" | "t.me" => "telegram",
        "wikipedia.org" => "wikipedia",
        h if h.ends_with(".bandcamp.com") => "bandcamp",
        h if h.ends_with(".wikipedia.org") => "wikipedia",
        h if h.ends_with(".allmusic.com") || h == "allmusic.com" => "allmusic",
        h if h.ends_with(".bandsintown.com") || h == "bandsintown.com" => "bandsintown",
        _ => return None,
    };
    Some(kind.to_string())
}

fn parse_sc_web_profiles(value: &Value) -> Vec<(String, String, String)> {
    let arr = match value.as_array() {
        Some(a) => a,
        None => match value.get("collection").and_then(|v| v.as_array()) {
            Some(a) => a,
            None => return Vec::new(),
        },
    };
    let mut out = Vec::new();
    for item in arr {
        let Some(url) = item.get("url").and_then(|v| v.as_str()) else {
            continue;
        };
        if url.is_empty() {
            continue;
        }
        let kind = classify_url(url).unwrap_or_else(|| {
            item.get("network")
                .or_else(|| item.get("service"))
                .and_then(|v| v.as_str())
                .unwrap_or("other")
                .to_string()
        });
        out.push((kind, url.to_string(), "sc".to_string()));
    }
    out
}

fn genius_socials(d: &GeniusArtistDetails, source: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    if let Some(name) = d.instagram.as_deref() {
        out.push((
            "instagram".to_string(),
            format!("https://instagram.com/{name}"),
            source.to_string(),
        ));
    }
    if let Some(name) = d.twitter.as_deref() {
        out.push((
            "twitter".to_string(),
            format!("https://twitter.com/{name}"),
            source.to_string(),
        ));
    }
    if let Some(name) = d.facebook.as_deref() {
        out.push((
            "facebook".to_string(),
            format!("https://facebook.com/{name}"),
            source.to_string(),
        ));
    }
    if let Some(genius_url) = d.url.as_deref() {
        if !genius_url.is_empty() {
            out.push((
                "genius".to_string(),
                genius_url.to_string(),
                source.to_string(),
            ));
        }
    }
    out
}
