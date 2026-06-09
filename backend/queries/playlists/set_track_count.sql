UPDATE playlists
SET track_count      = $2,
    tracks_synced_at = now(),
    updated_at       = now()
WHERE urn = $1
