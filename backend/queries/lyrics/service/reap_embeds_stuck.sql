SELECT sc_track_id, synced_lrc, plain_text, source, language, language_confidence, embedded_at
FROM lyrics_cache
WHERE embedded_at IS NULL
  AND created_at
    < $1
  AND length (coalesce (plain_text
    , synced_lrc
    , ''))
    > 30
ORDER BY created_at ASC LIMIT $2
