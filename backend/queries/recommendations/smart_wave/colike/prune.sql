DELETE
FROM artist_colike
WHERE updated_at < $1
