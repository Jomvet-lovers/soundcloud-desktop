SELECT DISTINCT d.sc_track_id
FROM disliked_tracks d
         JOIN tracks it ON it.sc_track_id = d.sc_track_id
WHERE it.indexed_at IS NOT NULL LIMIT $1
