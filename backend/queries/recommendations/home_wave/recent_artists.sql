SELECT LOWER(a.name) AS "name!"
FROM user_events ue
         JOIN tracks it ON it.sc_track_id = ue.sc_track_id
         JOIN track_artists ta ON ta.track_id = it.id AND ta.role = 'primary'
         JOIN artists a ON a.id = ta.artist_id
WHERE ue.sc_user_id = ANY ($1)
  AND ue.created_at > NOW() - INTERVAL '14 days'
ORDER BY ue.created_at DESC
    LIMIT $2
