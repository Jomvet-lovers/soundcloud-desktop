INSERT INTO album_artists (album_id, artist_id, role)
SELECT album_id, $1, role
FROM album_artists
WHERE artist_id = $2 ON CONFLICT DO NOTHING
