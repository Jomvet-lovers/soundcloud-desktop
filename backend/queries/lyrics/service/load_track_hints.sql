SELECT title, duration_ms, metadata_artist, uploader_username
FROM tracks
WHERE sc_track_id = $1
