INSERT INTO lyrics_cache (sc_track_id, synced_lrc, plain_text, source, language, language_confidence, embedded_at)
VALUES ($1, $2, $3, 'self_gen', NULL, NULL, NULL) ON CONFLICT (sc_track_id) DO NOTHING
RETURNING sc_track_id, synced_lrc, plain_text, source, language, language_confidence, embedded_at
