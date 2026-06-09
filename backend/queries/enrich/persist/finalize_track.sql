UPDATE tracks
SET primary_artist_id  = $2,
    album_id           = $3,
    isrc               = $4,
    canonical_track_id = COALESCE($5, canonical_track_id),
    cover_of_artist_id = $6,
    release_date       = COALESCE($10, release_date, sc_created_at::date),
    release_year       = COALESCE(
            $11,
            EXTRACT(YEAR FROM $10::date) ::smallint,
            release_year,
            EXTRACT(YEAR FROM sc_created_at) ::smallint
                         ),
    enrich_state       = 'done',
    enrich_source      = $7,
    enrich_confidence  = $8,
    enrich_attempts    = 0,
    enrich_locked_at   = NULL,
    enrich_error       = NULL,
    enriched_at        = now(),
    upload_kind        = $9
WHERE id = $1
