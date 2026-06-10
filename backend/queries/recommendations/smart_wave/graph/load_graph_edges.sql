SELECT a_id AS "a_id!", b_id AS "b_id!", weight::real AS "weight!", 0::int2 AS "kind!"
FROM artist_coplay
WHERE (a_id = ANY ($1) OR b_id = ANY ($1))
  AND weight > 0
UNION ALL
SELECT a_id, b_id, w, 1::int2
FROM artist_colike
WHERE (a_id = ANY ($1) OR b_id = ANY ($1))
  AND w > 0
