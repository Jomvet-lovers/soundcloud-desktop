SELECT wt.id, wt.title, wt.duration_ms, wt.release_year, wt.isrc, a.name AS "artist_name?"
FROM wanted_tracks wt
         LEFT JOIN artists a ON a.id = wt.primary_artist_id
WHERE wt.primary_artist_id = $1
  AND wt.track_id IS NULL
  AND wt.status = 'wanted'
ORDER BY wt.discovered_at DESC LIMIT $2
