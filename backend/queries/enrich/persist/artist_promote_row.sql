SELECT source, confidence, mb_artist_id, genius_artist_id, sc_user_id
FROM artists
WHERE id = $1
