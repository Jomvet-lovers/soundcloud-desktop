SELECT it.sc_track_id
FROM track_artists ta
         JOIN tracks it ON it.id = ta.track_id
         LEFT JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
WHERE ta.artist_id = $1
  AND ta.role = 'primary'
  AND it.sharing = 'public'
ORDER BY COALESCE(c.play_count, 0) DESC, it.created_at DESC LIMIT $2
