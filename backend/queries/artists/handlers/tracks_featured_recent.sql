SELECT t.sc_track_id
FROM track_artists ta
         JOIN tracks t ON t.id = ta.track_id
         LEFT JOIN sc_track_counters c ON c.sc_track_id = t.sc_track_id
WHERE ta.artist_id = $1
  AND ta.role IN ('featured', 'remixer')
ORDER BY t.release_date DESC NULLS LAST, t.release_year DESC NULLS LAST, t.created_at DESC, t.id DESC LIMIT $2
OFFSET $3
