SELECT a.id, a.name, a.avatar_url
FROM artist_coplay ac
         JOIN artists a ON a.id = CASE WHEN ac.a_id = $1 THEN ac.b_id ELSE ac.a_id END
WHERE (ac.a_id = $1 OR ac.b_id = $1)
  AND a.merged_into IS NULL
ORDER BY ac.weight DESC LIMIT $2
