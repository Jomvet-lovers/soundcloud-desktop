UPDATE playlists
SET track_count      = $2,
    tracks_synced_at = now(),
    desired_rev      = desired_rev + 1,
    updated_at       = now()
WHERE urn = $1 RETURNING desired_rev
