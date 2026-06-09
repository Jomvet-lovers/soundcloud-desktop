SELECT sc_track_id
FROM playlist_tracks
WHERE playlist_urn = $1
ORDER BY position
