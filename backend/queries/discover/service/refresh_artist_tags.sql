WITH per_artist AS (SELECT primary_artist_id  AS artist_id,
                           LOWER(TRIM(genre)) AS g,
                           COUNT(*)           AS cnt
                    FROM tracks
                    WHERE primary_artist_id IS NOT NULL
                      AND genre IS NOT NULL
                      AND TRIM(genre) <> ''
                    GROUP BY primary_artist_id, LOWER(TRIM(genre))),
     ranked AS (SELECT artist_id,
                       g,
                       ROW_NUMBER() OVER (PARTITION BY artist_id ORDER BY cnt DESC, g) AS rk
                FROM per_artist),
     top AS (SELECT artist_id,
                    ARRAY_AGG(g ORDER BY rk) FILTER (WHERE rk <= 3) AS tags
             FROM ranked
             GROUP BY artist_id),
     affected AS (SELECT artist_id
                  FROM top
                  UNION
                  SELECT id AS artist_id
                  FROM artists
                  WHERE array_length(tags, 1) IS NOT NULL
                    AND merged_into IS NULL)
UPDATE artists a
SET tags = COALESCE(t.tags, '{}'::text[]) FROM affected aff
LEFT JOIN top t
ON t.artist_id = aff.artist_id
WHERE a.id = aff.artist_id AND a.merged_into IS NULL
