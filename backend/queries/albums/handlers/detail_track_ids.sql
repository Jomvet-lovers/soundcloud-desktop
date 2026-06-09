SELECT t.sc_track_id
FROM album_tracks at
JOIN tracks t
ON t.id = at.track_id
WHERE at.album_id = $1
ORDER BY COALESCE (at.position, 32767), t.created_at
