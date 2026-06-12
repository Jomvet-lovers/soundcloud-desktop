-- Снять все кредиты артиста ($1) с треков, залитых аплоадером ($2).
DELETE
FROM track_artists ta
    USING tracks t
WHERE ta.track_id = t.id
  AND ta.artist_id = $1
  AND t.uploader_sc_user_id = $2
