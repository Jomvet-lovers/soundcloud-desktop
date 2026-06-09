WITH p30_listeners AS (SELECT it.primary_artist_id AS artist_id,
                              COUNT(DISTINCT ue.sc_user_id) ::bigint AS listeners
                       FROM user_events ue
                                JOIN tracks it ON it.sc_track_id = ue.sc_track_id
                       WHERE ue.event_type IN ('full_play', 'like', 'playlist_add')
                         AND ue.created_at > NOW() - INTERVAL '30 days'
    AND it.primary_artist_id IS NOT NULL
GROUP BY it.primary_artist_id
    ),
    p7_count AS (
SELECT it.primary_artist_id AS artist_id, COUNT (*)::bigint AS plays
FROM user_events ue
    JOIN tracks it
ON it.sc_track_id = ue.sc_track_id
WHERE ue.event_type = 'full_play'
  AND ue.created_at
    > NOW() - INTERVAL '7 days'
  AND it.primary_artist_id IS NOT NULL
GROUP BY it.primary_artist_id
    ),
    p30_count AS (
SELECT it.primary_artist_id AS artist_id, COUNT (*)::bigint AS plays
FROM user_events ue
    JOIN tracks it
ON it.sc_track_id = ue.sc_track_id
WHERE ue.event_type = 'full_play'
  AND ue.created_at
    > NOW() - INTERVAL '30 days'
  AND it.primary_artist_id IS NOT NULL
GROUP BY it.primary_artist_id
    ),
    affected AS (
SELECT artist_id
FROM p30_listeners
UNION
SELECT artist_id
FROM p30_count
UNION
SELECT id AS artist_id
FROM artists
WHERE (monthly_listeners
    > 0
   OR trending_score
    > 0)
  AND merged_into IS NULL
    )
UPDATE artists a
SET monthly_listeners = COALESCE(p30l.listeners, 0),
    trending_score    = LEAST(
            1.0::real,
            GREATEST(
                    0.0::real,
                    ((COALESCE(p7.plays, 0)::real * 30.0)
                         / (7.0 * (COALESCE(p30.plays, 0)::real + 1.0))
                        - 0.5
                        ) / 3.0
            )
                        ) FROM affected aff
LEFT JOIN p30_listeners p30l
ON p30l.artist_id = aff.artist_id
    LEFT JOIN p7_count p7 ON p7.artist_id = aff.artist_id
    LEFT JOIN p30_count p30 ON p30.artist_id = aff.artist_id
WHERE a.id = aff.artist_id AND a.merged_into IS NULL
