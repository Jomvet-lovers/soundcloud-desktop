SELECT wt.id,
       wt.title,
       COALESCE(a.name, '') AS "artist_name!",
       wt.duration_ms,
       wt.isrc,
       wt.primary_artist_id
FROM wanted_tracks wt
         LEFT JOIN artists a ON a.id = wt.primary_artist_id
WHERE wt.status = 'wanted'
  AND wt.track_id IS NULL
  AND wt.primary_artist_id = $2
ORDER BY wt.updated_at NULLS FIRST LIMIT $1
