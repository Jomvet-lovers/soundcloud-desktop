SELECT wt.id, wt.title, wt.duration_ms, wt.release_year, wt.primary_artist_id, a.name AS "name?", wta.position
FROM wanted_track_albums wta
         JOIN wanted_tracks wt ON wt.id = wta.wanted_track_id
         LEFT JOIN artists a ON a.id = wt.primary_artist_id
WHERE wta.album_id = $1
  AND wt.track_id IS NULL
  AND wt.status = 'wanted'
ORDER BY wta.position, wt.title
