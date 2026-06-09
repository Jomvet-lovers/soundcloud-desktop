SELECT sc_track_id
FROM tracks
WHERE storage_state = 'failed'
  AND updated_at < $1
ORDER BY updated_at
LIMIT $2
