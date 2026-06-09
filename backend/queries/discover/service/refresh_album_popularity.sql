WITH album_plays AS (SELECT at.album_id, SUM(COALESCE(c.play_count, 0)) ::bigint AS plays
                     FROM album_tracks at
    JOIN tracks it ON it.id = at.track_id
    LEFT JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id GROUP BY at.album_id
),
denom AS (
SELECT GREATEST(MAX (plays), 1)::bigint AS m
FROM album_plays
    ), affected AS (
SELECT album_id
FROM album_plays
WHERE plays > 0
UNION
SELECT id AS album_id
FROM albums
WHERE popularity_score
    > 0
    )
UPDATE albums al
SET popularity_score = LEAST(
        1.0::real,
        (LN(GREATEST(COALESCE(ap.plays, 0), 0) + 1)::real
         / NULLIF(LN((SELECT m FROM denom) + 1)::real, 0))
                       ) FROM affected aff
LEFT JOIN album_plays ap
ON ap.album_id = aff.album_id
WHERE al.id = aff.album_id
