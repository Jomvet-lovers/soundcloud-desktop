SELECT it.sc_track_id
FROM tracks it
         JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
WHERE it.indexed_at IS NOT NULL
  AND COALESCE(c.play_count, 0) >= $1
  AND COALESCE(c.likes_count, 0) >= $2
ORDER BY COALESCE(c.play_count, 0) DESC LIMIT 800
