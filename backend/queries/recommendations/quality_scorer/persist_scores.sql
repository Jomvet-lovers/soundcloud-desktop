UPDATE tracks AS it
SET quality_score = data.score FROM (SELECT * FROM UNNEST($1::text[], $2::real[]) AS t(sc_track_id, score)) AS data
WHERE it.sc_track_id = data.sc_track_id
