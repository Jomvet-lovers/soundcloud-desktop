INSERT INTO album_artists (album_id, artist_id, role)
VALUES ($1, $2, 'primary') ON CONFLICT DO NOTHING
