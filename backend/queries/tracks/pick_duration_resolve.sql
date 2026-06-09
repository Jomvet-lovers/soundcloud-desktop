SELECT sc_track_id
FROM tracks
WHERE needs_duration_resolve = true
ORDER BY sc_synced_at LIMIT $1
