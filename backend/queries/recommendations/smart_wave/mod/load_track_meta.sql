SELECT sc_track_id,
       primary_artist_id,
       (storage_state = 'ok') AS "ok!"
FROM tracks
WHERE sc_track_id = ANY ($1)
