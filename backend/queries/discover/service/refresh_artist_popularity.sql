WITH sc AS (SELECT t.primary_artist_id AS artist_id,
                   SUM(c.play_count) ::bigint AS plays
            FROM sc_track_counters c
                     JOIN tracks t ON t.sc_track_id = c.sc_track_id
            WHERE t.primary_artist_id IS NOT NULL
            GROUP BY t.primary_artist_id),
     internal AS (SELECT t.primary_artist_id AS artist_id,
                         COUNT(*) ::bigint AS fp
                  FROM user_events ue
                           JOIN tracks t ON t.sc_track_id = ue.sc_track_id
                  WHERE ue.event_type = 'full_play'
                    AND t.primary_artist_id IS NOT NULL
                  GROUP BY t.primary_artist_id),
     combined AS (SELECT COALESCE(sc.artist_id, internal.artist_id) AS artist_id,
                         COALESCE(sc.plays, 0)
                             + COALESCE(internal.fp, 0) * $1::bigint AS score
                  FROM sc
                           FULL OUTER JOIN internal ON sc.artist_id = internal.artist_id),
     denom AS (SELECT GREATEST(MAX(score), 1) ::bigint AS m
               FROM combined),
     affected AS (SELECT artist_id
                  FROM combined
                  WHERE score > 0
                  UNION
                  SELECT id AS artist_id
                  FROM artists
                  WHERE popularity_score > 0
                    AND merged_into IS NULL)
UPDATE artists a
SET popularity_score = LEAST(
        1.0::real,
        (LN(GREATEST(COALESCE(cm.score, 0), 0) + 1)::real
         / NULLIF(LN((SELECT m FROM denom) + 1)::real, 0))
                       ) FROM affected aff
LEFT JOIN combined cm
ON cm.artist_id = aff.artist_id
WHERE a.id = aff.artist_id AND a.merged_into IS NULL
