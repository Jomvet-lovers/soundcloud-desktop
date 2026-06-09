SELECT COUNT(*) ::int8 AS "count!"
FROM wanted_tracks
WHERE status = COALESCE($1, status)
