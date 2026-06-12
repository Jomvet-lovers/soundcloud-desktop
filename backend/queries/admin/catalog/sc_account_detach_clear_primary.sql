-- Обнулить денормализованный primary на треках аплоадера ($2), если он указывает
-- на отцепляемого артиста ($1).
UPDATE tracks
SET primary_artist_id = NULL
WHERE uploader_sc_user_id = $2
  AND primary_artist_id = $1
