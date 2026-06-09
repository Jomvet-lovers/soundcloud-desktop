-- stored tracks with no lyrics_cache past age → full self-gen; stale pending only
SELECT it.sc_track_id
FROM tracks it
         LEFT JOIN lyrics_cache lc ON lc.sc_track_id = it.sc_track_id
WHERE it.storage_state = 'ok'
  AND lc.sc_track_id IS NULL
  AND it.created_at < $1
  AND (it.transcribe_state IS NULL OR (it.transcribe_state = 'pending' AND it.transcribe_at < $3))
ORDER BY it.created_at ASC LIMIT $2
