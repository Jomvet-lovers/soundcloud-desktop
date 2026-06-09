SELECT title, type, release_year, cover_url, confidence, primary_artist_id
FROM albums
WHERE id = $1
