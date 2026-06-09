SELECT album_id, position
FROM wanted_track_albums
WHERE wanted_track_id = $1
