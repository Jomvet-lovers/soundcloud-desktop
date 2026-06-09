SELECT DISTINCT
ON (ta.artist_id, it.sc_track_id)
    ta.artist_id, it.sc_track_id
FROM track_artists ta
    JOIN tracks it
ON it.id = ta.track_id
    LEFT JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
WHERE ta.artist_id = ANY ($1)
  AND ta.role = 'primary'
  AND it.sharing = 'public'
ORDER BY ta.artist_id, it.sc_track_id, COALESCE (c.play_count, 0) DESC
    LIMIT $2
