SELECT COUNT(*) FILTER (WHERE status = 'wanted' AND track_id IS NULL)::int8 AS "wanted!", COUNT(*) FILTER (WHERE status = 'unresolvable')::int8 AS "unresolvable!"
FROM wanted_tracks
