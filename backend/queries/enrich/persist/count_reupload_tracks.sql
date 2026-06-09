SELECT COUNT(DISTINCT it.id) ::int8 AS "count!"
FROM tracks it
         JOIN track_artists ta ON ta.track_id = it.id AND ta.role = 'primary'
WHERE it.uploader_sc_user_id = $1
  AND ta.artist_id = $2
