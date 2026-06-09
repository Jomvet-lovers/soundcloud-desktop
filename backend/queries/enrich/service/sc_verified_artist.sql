SELECT a.id, a.name, a.mb_artist_id, a.genius_artist_id
FROM artist_sc_accounts asa
         JOIN artists a ON a.id = asa.artist_id
WHERE asa.sc_user_id = $1
  AND a.merged_into IS NULL LIMIT 1
