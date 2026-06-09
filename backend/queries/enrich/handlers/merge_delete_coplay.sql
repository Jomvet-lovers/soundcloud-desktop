DELETE
FROM artist_coplay
WHERE a_id = $1
   OR b_id = $1
