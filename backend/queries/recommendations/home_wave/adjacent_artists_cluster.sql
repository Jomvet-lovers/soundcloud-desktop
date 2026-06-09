WITH recent_likes AS (SELECT sc_track_id
                      FROM user_likes_tracks
                      WHERE user_id = ANY ($1)
                        AND wanted_state = true
                        AND created_at > NOW() - INTERVAL '180 days'
ORDER BY created_at DESC, ctid DESC
    LIMIT 300
    ),
    user_artists AS (
SELECT DISTINCT ta.artist_id
FROM recent_likes rl
    JOIN tracks it
ON it.sc_track_id = rl.sc_track_id
    JOIN track_artists ta ON ta.track_id = it.id AND ta.role = 'primary'
    LIMIT 100
    ),
    co AS (
SELECT
    (CASE WHEN ac.a_id IN (SELECT artist_id FROM user_artists)
    THEN ac.b_id ELSE ac.a_id END) AS co_id, MAX (ac.weight) AS w
FROM artist_coplay ac
WHERE (ac.a_id IN (SELECT artist_id FROM user_artists)
   OR ac.b_id IN (SELECT artist_id FROM user_artists))
  AND NOT (
    ac.a_id IN (SELECT artist_id FROM user_artists)
  AND ac.b_id IN (SELECT artist_id FROM user_artists)
    )
GROUP BY co_id
ORDER BY w DESC
    LIMIT $2
    ),
    ranked AS (
SELECT
    ta.artist_id, it.sc_track_id, ROW_NUMBER() OVER (
    PARTITION BY ta.artist_id
    ORDER BY
    CASE WHEN it.sc_track_id = ANY ($3) THEN 1 ELSE 0 END, COALESCE (c.play_count, 0) DESC
    ) AS rn, co.w
FROM co
    JOIN track_artists ta
ON ta.artist_id = co.co_id AND ta.role = 'primary'
    JOIN tracks it ON it.id = ta.track_id
    LEFT JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
WHERE it.sharing = 'public'
  AND it.storage_state = 'ok'
    )
SELECT a.id AS "artist_id!", a.name AS "artist_name!", a.avatar_url, r.sc_track_id AS "sc_track_id!"
FROM ranked r
         JOIN artists a ON a.id = r.artist_id
WHERE r.rn = 1
  AND a.merged_into IS NULL
ORDER BY r.w DESC NULLS LAST
