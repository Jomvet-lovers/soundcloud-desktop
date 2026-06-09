SELECT a_id, b_id, weight
FROM artist_coplay
WHERE (a_id = ANY ($1) OR b_id = ANY ($1))
  AND weight > 0
