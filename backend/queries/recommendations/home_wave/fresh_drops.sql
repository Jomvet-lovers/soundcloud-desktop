WITH recent_likes AS (SELECT sc_track_id
                      FROM user_likes_tracks
                      WHERE user_id = ANY ($1)
                        AND wanted_state = true
                        AND created_at > NOW() - INTERVAL '120 days'
ORDER BY created_at DESC, ctid DESC
    LIMIT 200
    ),
    user_artists AS (
SELECT ta.artist_id
FROM recent_likes rl
    JOIN tracks it
ON it.sc_track_id = rl.sc_track_id
    JOIN track_artists ta ON ta.track_id = it.id AND ta.role = 'primary'
GROUP BY ta.artist_id
HAVING COUNT (*) >= 2
    )
SELECT it.sc_track_id AS "sc_track_id!"
FROM track_artists ta
         JOIN tracks it ON it.id = ta.track_id
WHERE ta.artist_id IN (SELECT artist_id FROM user_artists)
  AND ta.role = 'primary'
  AND it.sharing = 'public'
  AND it.storage_state = 'ok'
  AND it.sc_synced_at > NOW() - INTERVAL '30 days'
  AND NOT (it.sc_track_id = ANY ($2))
ORDER BY it.sc_synced_at DESC
    LIMIT $3
