SELECT (SELECT COUNT(*) ::bigint
        FROM artists
        WHERE merged_into IS NULL
          AND (track_count_primary > 0 OR track_count_featured > 0))        AS "artists_count!",
       (SELECT COUNT(*) ::bigint
        FROM albums
        WHERE track_count > 0
          AND popularity_score > 0
          AND primary_artist_id IS NOT NULL)                                AS "albums_count!",
       (SELECT COUNT(*) ::bigint
        FROM albums
        WHERE track_count > 0
          AND popularity_score > 0
          AND primary_artist_id IS NOT NULL
          AND release_date IS NOT NULL
          AND release_date > (CURRENT_DATE - ($1::int * INTERVAL '1 day'))) AS "fresh_count!"
