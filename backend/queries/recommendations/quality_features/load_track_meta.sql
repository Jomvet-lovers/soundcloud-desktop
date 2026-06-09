SELECT it.sc_track_id,
       it.title,
       it.genre,
       it.duration_ms,
       c.play_count  AS "play_count?",
       c.likes_count AS "likes_count?"
FROM tracks it
         LEFT JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
WHERE it.sc_track_id = ANY ($1)
