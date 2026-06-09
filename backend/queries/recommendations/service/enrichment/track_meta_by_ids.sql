SELECT sc_track_id, uploader_username, genre, language, play_count_sc
FROM tracks
WHERE sc_track_id = ANY ($1)
