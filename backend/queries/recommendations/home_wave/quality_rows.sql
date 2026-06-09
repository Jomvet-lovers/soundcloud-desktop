SELECT it.sc_track_id AS "sc_track_id!",
       it.duration_ms AS "duration_ms!",
       it.title       AS "title!",
       c.play_count,
       it.quality_score
FROM tracks it
         LEFT JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
WHERE it.sc_track_id = ANY ($1)
