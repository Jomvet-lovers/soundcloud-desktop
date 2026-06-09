WITH rl AS (SELECT sc_track_id,
                   EXP(-EXTRACT(EPOCH FROM (now() - created_at)) / 86400.0 / 60.0) ::real AS rec
            FROM user_likes_tracks
            WHERE user_id = ANY ($1)
              AND wanted_state = true
              AND created_at > now() - make_interval(days => $2::int)
            ORDER BY created_at DESC, ctid DESC
    LIMIT 200
    )
   , parts AS (
SELECT ta.artist_id, rl.rec, (CASE ta.role WHEN 'primary' THEN 1.0 WHEN 'featured' THEN 0.6 ELSE 0.5 END):: real AS rw
FROM rl
    JOIN tracks it
ON it.sc_track_id = rl.sc_track_id
    JOIN track_artists ta ON ta.track_id = it.id AND ta.role IN ('primary','featured','remixer')
UNION ALL
SELECT aa.artist_id, rl.rec, 0.8:: real
FROM rl
    JOIN tracks it
ON it.sc_track_id = rl.sc_track_id
    JOIN album_artists aa ON aa.album_id = it.album_id
WHERE NOT EXISTS (
    SELECT 1 FROM track_artists ta2
    WHERE ta2.track_id = it.id
  AND ta2.role IN ('primary'
    , 'featured'
    , 'remixer')
    )
    )
    , like_w AS (
SELECT artist_id, SUM (rec*rw):: real AS w
FROM parts
GROUP BY artist_id),
    play_w AS (
SELECT ta.artist_id, SUM (EXP(- EXTRACT (EPOCH FROM (now()-ue.created_at))/86400.0/60.0)):: real AS w
FROM user_events ue
    JOIN tracks it
ON it.sc_track_id = ue.sc_track_id
    JOIN track_artists ta ON ta.track_id = it.id AND ta.role = 'primary'
WHERE ue.sc_user_id = ANY ($1)
  AND ue.event_type IN ('full_play'
    , 'play_complete')
  AND ue.created_at
    > now() - make_interval(days => $3:: int)
GROUP BY ta.artist_id
    ),
    merged AS (
SELECT COALESCE (l.artist_id, p.artist_id) AS artist_id, (COALESCE (l.w, 0) + $4:: real * COALESCE (p.w, 0)):: real AS weight
FROM like_w l FULL OUTER JOIN play_w p
ON p.artist_id = l.artist_id
    )
SELECT m.artist_id AS "artist_id!", m.weight AS "weight!"
FROM merged m
         JOIN artists a ON a.id = m.artist_id
WHERE a.merged_into IS NULL
  AND m.weight > 0
ORDER BY m.weight DESC LIMIT $5
