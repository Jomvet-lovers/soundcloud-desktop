SELECT t.sc_track_id
FROM track_artists ta
         JOIN tracks t ON t.id = ta.track_id
         LEFT JOIN sc_track_counters c ON c.sc_track_id = t.sc_track_id
WHERE ta.artist_id = $1
ORDER BY COALESCE(c.play_count, 0) DESC, t.created_at DESC LIMIT $2
OFFSET $3
