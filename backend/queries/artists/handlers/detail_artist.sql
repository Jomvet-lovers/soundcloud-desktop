SELECT name, country, bio, avatar_url, confidence
FROM artists
WHERE id = $1
  AND merged_into IS NULL
