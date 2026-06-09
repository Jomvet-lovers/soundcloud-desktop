WITH ranked AS (SELECT ta.artist_id,
                       it.sc_track_id,
                       ROW_NUMBER() OVER (
            PARTITION BY ta.artist_id ORDER BY COALESCE(c.play_count, 0) DESC
        ) AS rn
                FROM track_artists ta
                         JOIN tracks it ON it.id = ta.track_id
                         LEFT JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
                WHERE ta.artist_id = ANY ($1)
                  AND ta.role = 'primary'
                  AND it.sharing = 'public'
                  AND it.storage_state = 'ok'
                  AND it.index_state = 'indexed'
                  AND NOT (it.sc_track_id = ANY ($2)))
SELECT artist_id AS "artist_id!", sc_track_id AS "sc_track_id!"
FROM ranked
WHERE rn <= $3 LIMIT $4
