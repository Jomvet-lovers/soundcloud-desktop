SELECT sc_track_id, play_count, likes_count, reposts_count, comment_count, fetched_at
FROM sc_track_counters
WHERE sc_track_id = ANY ($1)
