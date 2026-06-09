WITH primary_counts AS (SELECT artist_id, COUNT(*) ::int AS n
                        FROM track_artists
                        WHERE role = 'primary'
                        GROUP BY artist_id),
     featured_counts AS (SELECT artist_id, COUNT(DISTINCT track_id) ::int AS n
                         FROM track_artists
                         WHERE role IN ('featured', 'remixer')
                         GROUP BY artist_id),
     album_total AS (SELECT artist_id, COUNT(DISTINCT album_id) ::int AS n
                     FROM (SELECT primary_artist_id AS artist_id, id AS album_id
                           FROM albums
                           WHERE primary_artist_id IS NOT NULL
                           UNION
                           SELECT artist_id, album_id
                           FROM album_artists) x
                     GROUP BY artist_id),
     affected AS (SELECT artist_id
                  FROM primary_counts
                  UNION
                  SELECT artist_id
                  FROM featured_counts
                  UNION
                  SELECT artist_id
                  FROM album_total
                  UNION
                  SELECT id AS artist_id
                  FROM artists
                  WHERE (track_count_primary > 0
                      OR track_count_featured > 0
                      OR album_count_denorm > 0)
                    AND merged_into IS NULL)
UPDATE artists a
SET track_count_primary   = COALESCE(pc.n, 0),
    track_count_featured  = COALESCE(fc.n, 0),
    album_count_denorm    = COALESCE(at_.n, 0),
    aggregates_updated_at = NOW() FROM affected aff
LEFT JOIN primary_counts  pc
ON pc.artist_id = aff.artist_id
    LEFT JOIN featured_counts fc ON fc.artist_id = aff.artist_id
    LEFT JOIN album_total at_ ON at_.artist_id = aff.artist_id
WHERE a.id = aff.artist_id AND a.merged_into IS NULL
