INSERT INTO lyrics_cache (sc_track_id, synced_lrc, plain_text, source, language, language_confidence, embedded_at)
VALUES ($1, $2, $3, $4, NULL, NULL,
        NULL) RETURNING sc_track_id, synced_lrc, plain_text, source, language, language_confidence, embedded_at
