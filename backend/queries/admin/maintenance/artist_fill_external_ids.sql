UPDATE artists
SET mb_artist_id     = COALESCE(mb_artist_id, $2),
    genius_artist_id = COALESCE(genius_artist_id, $3)
WHERE id = $1
