SELECT it.id, it.sc_track_id, COALESCE(it.title, '') AS "title!"
FROM tracks it
         JOIN track_artists ta ON ta.track_id = it.id
WHERE ta.artist_id = $1
  AND ta.role = 'primary'
  AND (it.title_normalized = $2 OR it.title_normalized LIKE $3)
