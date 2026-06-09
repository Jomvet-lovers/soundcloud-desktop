SELECT sc_track_id, synced_lrc, plain_text, source, language, language_confidence, embedded_at
FROM lyrics_cache
WHERE sc_track_id = $1
