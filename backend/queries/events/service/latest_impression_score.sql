SELECT score
FROM rec_impressions
WHERE sc_user_id = ANY ($1)
  AND sc_track_id = $2
ORDER BY shown_at DESC LIMIT 1
