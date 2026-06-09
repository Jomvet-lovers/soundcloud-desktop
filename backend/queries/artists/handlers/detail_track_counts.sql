SELECT (SELECT COUNT(*) ::bigint
        FROM track_artists
        WHERE artist_id = $1
          AND role = 'primary')                AS "primary!",
       (SELECT COUNT(DISTINCT track_id) ::bigint
        FROM track_artists
        WHERE artist_id = $1
          AND role IN ('featured', 'remixer')) AS "featured!"
