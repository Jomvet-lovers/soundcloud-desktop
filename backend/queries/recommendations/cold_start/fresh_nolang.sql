SELECT sc_track_id
FROM tracks
WHERE sc_synced_at > NOW() - make_interval(days => $1::int)
  AND sharing = 'public'
ORDER BY sc_synced_at DESC LIMIT $2
