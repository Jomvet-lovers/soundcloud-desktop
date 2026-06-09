WITH album_meta AS (SELECT at.album_id,
                           COUNT(*)::int AS track_count, COALESCE(SUM(it.duration_ms), 0)::bigint AS total_ms, MIN(it.release_date) AS earliest_release
                    FROM album_tracks at
    JOIN tracks it ON it.id = at.track_id GROUP BY at.album_id
),
affected AS (
SELECT album_id
FROM album_meta
UNION
SELECT id AS album_id
FROM albums
WHERE track_count
    > 0
   OR total_duration_ms
    > 0
    )
UPDATE albums al
SET track_count           = COALESCE(am.track_count, 0),
    total_duration_ms     = COALESCE(am.total_ms, 0),
    release_date          = COALESCE(am.earliest_release, al.release_date),
    aggregates_updated_at = NOW() FROM affected aff
LEFT JOIN album_meta am
ON am.album_id = aff.album_id
WHERE al.id = aff.album_id
