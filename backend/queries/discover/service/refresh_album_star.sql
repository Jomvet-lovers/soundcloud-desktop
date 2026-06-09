WITH affected AS (SELECT id AS album_id, primary_artist_id
                  FROM albums
                  WHERE primary_artist_id IS NOT NULL
                    AND (
                      is_star_artist = TRUE
                          OR EXISTS (SELECT 1
                                     FROM artists a
                                     WHERE a.id = albums.primary_artist_id
                                       AND a.is_star = TRUE)
                      ))
UPDATE albums al
SET is_star_artist = COALESCE(a.is_star, false) FROM affected aff
LEFT JOIN artists a
ON a.id = aff.primary_artist_id
WHERE al.id = aff.album_id
