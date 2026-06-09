INSERT INTO playlist_tracks (playlist_urn, position, sc_track_id)
SELECT p, pos, t
FROM UNNEST($1::text[], $2::int[], $3::text[]) AS u(p, pos, t)
