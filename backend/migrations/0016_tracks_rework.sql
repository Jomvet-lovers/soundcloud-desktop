-- Cold-storage rework: финальная нормализованная схема.

DROP TABLE IF EXISTS cached_playlist_tracks CASCADE;
DROP TABLE IF EXISTS cached_playlists CASCADE;
DROP TABLE IF EXISTS cached_users CASCADE;
DROP TABLE IF EXISTS indexed_tracks CASCADE;

ALTER TABLE track_artists DROP CONSTRAINT IF EXISTS track_artists_pkey;
ALTER TABLE track_artists DROP COLUMN IF EXISTS indexed_track_id;

ALTER TABLE album_tracks DROP CONSTRAINT IF EXISTS album_tracks_pkey;
ALTER TABLE album_tracks DROP COLUMN IF EXISTS indexed_track_id;

ALTER TABLE wanted_tracks DROP COLUMN IF EXISTS indexed_track_id;

CREATE TABLE tracks (
    id                       uuid PRIMARY KEY DEFAULT gen_random_uuid(),

    sc_track_id              text NOT NULL,
    urn                      text NOT NULL,

    title                    text NOT NULL,
    title_normalized         text NOT NULL,
    description              text,
    genre                    text,
    tags                     text[] NOT NULL DEFAULT '{}',
    duration_ms              integer NOT NULL,
    artwork_url              text,
    permalink_url            text,
    waveform_url             text,
    language                 varchar(8),
    language_confidence      real,
    isrc                     text,
    metadata_artist          text,
    sharing                  varchar(8) NOT NULL DEFAULT 'public',
    sc_created_at            timestamptz,
    sc_last_modified         timestamptz,
    release_year             smallint,
    release_date             date,

    uploader_sc_user_id      text,
    uploader_urn             text,
    uploader_username        text,
    uploader_avatar_url      text,

    primary_artist_id        uuid REFERENCES artists(id),
    album_id                 uuid REFERENCES albums(id),
    album_position           smallint,
    canonical_track_id       uuid,
    upload_kind              varchar(16) NOT NULL DEFAULT 'unknown',

    audio_fingerprint        text,
    quality_score            real,
    play_count_sc            bigint,
    likes_count_sc           bigint,
    reposts_count_sc         bigint,
    comments_count_sc        bigint,

    enrich_state             varchar(16) NOT NULL DEFAULT 'pending',
    enrich_attempts          smallint NOT NULL DEFAULT 0,
    enrich_source            varchar(16),
    enrich_confidence        real,
    enriched_at              timestamptz,

    -- index_priority: 1=like, 2=playlist, 3=played, 4=fresh, 5=discovery.
    -- qdrant сам фильтрует "есть ли вектор" — `indexed_at` это просто отметка.
    index_state              varchar(16) NOT NULL DEFAULT 'pending',
    index_priority           smallint NOT NULL DEFAULT 5,
    index_attempts           smallint NOT NULL DEFAULT 0,
    indexed_at               timestamptz,

    -- storage_state ∈ {'pending','ok','failed','missing'}.
    -- storage_quality ∈ {'sq','hq'} независимо: state описывает наличие
    -- файла в S3, quality — какое именно качество там лежит.
    -- hq_upgrade_pending взводится при sq-приземлении, снимается при hq.
    -- streaming-cron подбирает кандидатов FOR UPDATE SKIP LOCKED.
    storage_state            varchar(16) NOT NULL DEFAULT 'pending',
    storage_priority         smallint NOT NULL DEFAULT 5,
    storage_quality          varchar(4),
    storage_attempts         smallint NOT NULL DEFAULT 0,
    s3_verified_at           timestamptz,
    s3_missing_at            timestamptz,
    hq_upgrade_pending       boolean NOT NULL DEFAULT false,
    hq_upgrade_attempts      smallint NOT NULL DEFAULT 0,
    hq_upgrade_last_at       timestamptz,

    -- SC иногда отдаёт duration=30000 без full_duration. Cron перечитывает
    -- через apiv2 и сбрасывает флаг.
    needs_duration_resolve   boolean NOT NULL DEFAULT false,

    sc_synced_at             timestamptz NOT NULL DEFAULT now(),
    last_read_at             timestamptz,

    created_at               timestamptz NOT NULL DEFAULT now(),
    updated_at               timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX tracks_sc_track_id_uq        ON tracks (sc_track_id);
CREATE UNIQUE INDEX tracks_urn_uq                ON tracks (urn);
CREATE INDEX tracks_primary_artist_idx           ON tracks (primary_artist_id)
                                                 WHERE primary_artist_id IS NOT NULL;
CREATE INDEX tracks_album_idx                    ON tracks (album_id) WHERE album_id IS NOT NULL;
CREATE INDEX tracks_canonical_idx                ON tracks (canonical_track_id)
                                                 WHERE canonical_track_id IS NOT NULL;
CREATE INDEX tracks_uploader_idx                 ON tracks (uploader_sc_user_id)
                                                 WHERE uploader_sc_user_id IS NOT NULL;
CREATE INDEX tracks_uploader_artist_idx          ON tracks (uploader_sc_user_id, primary_artist_id)
                                                 WHERE uploader_sc_user_id IS NOT NULL
                                                   AND primary_artist_id IS NOT NULL;
CREATE INDEX tracks_isrc_idx                     ON tracks (isrc) WHERE isrc IS NOT NULL;
CREATE INDEX tracks_title_norm_idx               ON tracks (title_normalized);
CREATE INDEX tracks_release_year_idx             ON tracks (release_year)
                                                 WHERE release_year IS NOT NULL;
CREATE INDEX tracks_release_date_idx             ON tracks (release_date DESC NULLS LAST)
                                                 WHERE release_date IS NOT NULL;
CREATE INDEX tracks_enrich_pickup_idx            ON tracks (enriched_at NULLS FIRST, enrich_attempts)
                                                 WHERE enrich_state IN ('pending','failed');
CREATE INDEX tracks_index_pickup_idx             ON tracks (index_priority, indexed_at NULLS FIRST, index_attempts)
                                                 WHERE index_state IN ('pending','failed');
CREATE INDEX tracks_storage_pending_idx          ON tracks (storage_priority, created_at)
                                                 WHERE storage_state = 'pending';
CREATE INDEX tracks_hq_upgrade_idx               ON tracks (hq_upgrade_last_at NULLS FIRST)
                                                 WHERE hq_upgrade_pending = true;
CREATE INDEX tracks_duration_resolve_idx         ON tracks (sc_synced_at)
                                                 WHERE needs_duration_resolve = true;
CREATE INDEX tracks_audio_fingerprint_prefix_idx ON tracks USING btree (substr(audio_fingerprint, 1, 64))
                                                 WHERE audio_fingerprint IS NOT NULL;
CREATE INDEX tracks_synced_at_idx                ON tracks (sc_synced_at);
CREATE INDEX tracks_last_read_at_idx             ON tracks (last_read_at);
CREATE INDEX tracks_quality_score_idx            ON tracks (quality_score)
                                                 WHERE quality_score IS NOT NULL;

ALTER TABLE track_artists
    ADD COLUMN track_id uuid NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    ADD PRIMARY KEY (track_id, artist_id, role);
CREATE INDEX track_artists_track_idx ON track_artists (track_id);

ALTER TABLE album_tracks
    ADD COLUMN track_id uuid NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    ADD PRIMARY KEY (album_id, track_id);
CREATE INDEX album_tracks_track_id_idx ON album_tracks (track_id);

ALTER TABLE wanted_tracks
    ADD COLUMN track_id uuid REFERENCES tracks(id) ON DELETE SET NULL;
CREATE INDEX wanted_tracks_track_id_idx ON wanted_tracks (track_id) WHERE track_id IS NOT NULL;

CREATE TABLE users (
    sc_user_id          text PRIMARY KEY,
    urn                 text NOT NULL UNIQUE,
    username            text NOT NULL,
    username_normalized text NOT NULL,
    full_name           text,
    first_name          text,
    last_name           text,
    permalink           text,
    permalink_url       text,
    avatar_url          text,
    country             varchar(8),
    city                text,
    description         text,
    verified            boolean NOT NULL DEFAULT false,
    followers_count     bigint,
    followings_count    bigint,
    tracks_count        bigint,
    playlists_count     bigint,
    reposts_count       bigint,
    comments_count      bigint,
    kind                varchar(16),
    sc_created_at       timestamptz,
    sc_last_modified    timestamptz,

    sc_synced_at        timestamptz NOT NULL DEFAULT now(),
    last_read_at        timestamptz,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX users_username_norm_idx ON users (username_normalized);
CREATE INDEX users_synced_at_idx     ON users (sc_synced_at);
CREATE INDEX users_last_read_at_idx  ON users (last_read_at);

CREATE TABLE playlists (
    urn                 text PRIMARY KEY,
    sc_playlist_id      text NOT NULL UNIQUE,
    title               text NOT NULL,
    title_normalized    text NOT NULL,
    description         text,
    genre               text,
    tags                text[] NOT NULL DEFAULT '{}',
    artwork_url         text,
    permalink_url       text,
    owner_sc_user_id    text,
    owner_urn           text,
    owner_username      text,
    track_count         integer NOT NULL DEFAULT 0,
    duration_ms         bigint,
    playlist_type       varchar(32),
    kind                varchar(16),
    sharing             varchar(8) NOT NULL DEFAULT 'public',
    release_year        smallint,
    release_date        date,
    label_name          text,
    sc_created_at       timestamptz,
    sc_last_modified    timestamptz,

    tracks_synced_at    timestamptz,
    sc_synced_at        timestamptz NOT NULL DEFAULT now(),
    last_read_at        timestamptz,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX playlists_owner_idx        ON playlists (owner_sc_user_id) WHERE owner_sc_user_id IS NOT NULL;
CREATE INDEX playlists_title_norm_idx   ON playlists (title_normalized);
CREATE INDEX playlists_kind_idx         ON playlists (kind) WHERE kind IS NOT NULL;
CREATE INDEX playlists_release_year_idx ON playlists (release_year) WHERE release_year IS NOT NULL;
CREATE INDEX playlists_synced_at_idx    ON playlists (sc_synced_at);
CREATE INDEX playlists_last_read_at_idx ON playlists (last_read_at);

CREATE TABLE playlist_tracks (
    playlist_urn  text NOT NULL,
    position      integer NOT NULL,
    sc_track_id   text NOT NULL,
    PRIMARY KEY (playlist_urn, position)
);
CREATE INDEX playlist_tracks_track_idx    ON playlist_tracks (sc_track_id);
CREATE INDEX playlist_tracks_playlist_idx ON playlist_tracks (playlist_urn);

-- Client Credentials пул. PK на oauth_app_id — один токен на аппку
-- (SC reuse-политика: 50/12h/app, 30/1h/IP).
-- access_token=NULL означает "refresh fell в circuit-breaker" — токена нет,
-- но строка живёт для refresh_attempts/last_refresh_error. snapshot() и read'ы
-- фильтруют через `access_token IS NOT NULL`.
CREATE TABLE oauth_app_tokens (
    oauth_app_id       uuid PRIMARY KEY REFERENCES oauth_apps(id) ON DELETE CASCADE,
    access_token       text,
    scope              text,
    expires_at         timestamptz NOT NULL,
    last_used_at       timestamptz,
    refreshed_at       timestamptz NOT NULL DEFAULT now(),
    refresh_attempts   integer NOT NULL DEFAULT 0,
    last_refresh_error text
);
CREATE INDEX oauth_app_tokens_pickup_idx ON oauth_app_tokens (last_used_at NULLS FIRST, expires_at DESC);
CREATE INDEX oauth_app_tokens_expiry_idx ON oauth_app_tokens (expires_at);

ALTER TABLE artists ADD COLUMN last_account_walk_at timestamptz;
CREATE INDEX artists_account_walk_pickup_idx
    ON artists (last_account_walk_at NULLS FIRST)
    WHERE merged_into IS NULL;
