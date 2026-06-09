SELECT w.id,
       w.title,
       w.status,
       w.source,
       w.external_id,
       w.isrc,
       w.release_year,
       w.primary_artist_id,
       a.name AS "primary_artist_name?",
       w.track_id,
       w.resolve_attempts,
       w.resolve_error,
       w.discovered_at,
       w.updated_at
FROM wanted_tracks w
         LEFT JOIN artists a ON a.id = w.primary_artist_id
WHERE w.status = COALESCE($1, w.status)
ORDER BY w.discovered_at DESC LIMIT $2
OFFSET $3
