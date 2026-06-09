SELECT it.sc_track_id
FROM tracks it
         JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
WHERE it.language = ANY ($1)
  AND it.sharing = 'public'
ORDER BY COALESCE(c.play_count, 0) DESC LIMIT $2
