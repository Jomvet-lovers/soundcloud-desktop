WITH anchor_artists AS (SELECT artist_id
                        FROM track_artists ta
                                 JOIN tracks it ON it.id = ta.track_id
                        WHERE it.sc_track_id = $1),
     feat_artists AS (SELECT DISTINCT ta.artist_id
                      FROM track_artists ta
                               JOIN tracks it ON it.id = ta.track_id
                      WHERE ta.role IN ('featured', 'remixer')
                        AND it.id IN (SELECT track_id
                                      FROM track_artists
                                      WHERE artist_id IN (SELECT artist_id FROM anchor_artists))
                        AND ta.artist_id NOT IN (SELECT artist_id FROM anchor_artists)),
     ranked AS (SELECT ta.artist_id,
                       it.sc_track_id,
                       ROW_NUMBER() OVER (
            PARTITION BY ta.artist_id
            ORDER BY COALESCE(c.play_count, 0) DESC
        ) AS rn
                FROM feat_artists fa
                         JOIN track_artists ta ON ta.artist_id = fa.artist_id AND ta.role = 'primary'
                         JOIN tracks it ON it.id = ta.track_id
                         LEFT JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
                WHERE it.sc_track_id <> $1
                  AND it.sharing = 'public')
SELECT a.id AS artist_id, a.name AS artist_name, a.avatar_url, r.sc_track_id AS "sc_track_id!"
FROM ranked r
         JOIN artists a ON a.id = r.artist_id
WHERE r.rn = 1
  AND a.merged_into IS NULL LIMIT $2
