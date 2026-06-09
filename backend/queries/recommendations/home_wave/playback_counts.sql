SELECT sc_track_id AS "sc_track_id!", play_count
FROM sc_track_counters
WHERE sc_track_id = ANY ($1)
