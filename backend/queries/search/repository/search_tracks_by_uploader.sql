-- Per-user track search: uploader filter first, then trgm. Columns in TrackRow order.
SELECT id,
       sc_track_id,
       urn,
       title,
       title_normalized,
       description,
       genre,
       tags,
       duration_ms,
       artwork_url,
       permalink_url,
       waveform_url, language, language_confidence, isrc, metadata_artist, sharing, sc_created_at, sc_last_modified, release_year, release_date, uploader_sc_user_id, uploader_urn, uploader_username, uploader_avatar_url, primary_artist_id, album_id, album_position, canonical_track_id, cover_of_artist_id, upload_kind, audio_fingerprint, quality_score, play_count_sc, likes_count_sc, reposts_count_sc, comments_count_sc, enrich_state, enrich_attempts, enrich_source, enrich_confidence, enriched_at, index_state, index_priority, index_attempts, indexed_at, storage_state, storage_priority, storage_quality, storage_attempts, s3_verified_at, s3_missing_at, hq_upgrade_pending, hq_upgrade_attempts, hq_upgrade_last_at, needs_duration_resolve, sc_synced_at, last_read_at, created_at, updated_at
FROM tracks
WHERE uploader_sc_user_id = $1
  AND sharing = 'public'
  AND (title_normalized LIKE $5
   OR LOWER (title) LIKE $2)
ORDER BY play_count_sc DESC NULLS LAST, sc_synced_at DESC, id DESC
    LIMIT $3
OFFSET $4
