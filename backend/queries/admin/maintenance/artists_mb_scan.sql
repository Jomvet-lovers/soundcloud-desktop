SELECT id, name, normalized_name, mb_artist_id AS "mb_artist_id!", genius_artist_id
FROM artists
WHERE source = 'mb'
  AND mb_artist_id IS NOT NULL
  AND merged_into IS NULL
  AND id > $1
ORDER BY id
LIMIT $2
