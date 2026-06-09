WITH pairs AS (SELECT a.artist_id AS aid, b.artist_id AS bid
               FROM track_artists a
                        JOIN track_artists b
                             ON a.track_id = b.track_id
                                 AND a.artist_id <> b.artist_id
               WHERE a.track_id = $1),
     normalized AS (SELECT LEAST(aid, bid) AS a, GREATEST(aid, bid) AS b
                    FROM pairs),
     deduped AS (SELECT DISTINCT a, b
                 FROM normalized)
INSERT
INTO artist_coplay (a_id, b_id, weight, last_seen)
SELECT a, b, 1, now()
FROM deduped ON CONFLICT (a_id, b_id) DO
UPDATE
    SET weight = artist_coplay.weight + 1,
    last_seen = now()
