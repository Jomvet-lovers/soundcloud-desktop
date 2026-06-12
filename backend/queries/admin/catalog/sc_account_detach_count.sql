-- Сколько треков аплоадера ($2) сейчас залинковано на артиста ($1) — как кредит
-- или как денормализованный primary. Считаем ДО отцепа, чтобы вернуть количество.
SELECT COUNT(*)::int8 AS "count!"
FROM tracks t
WHERE t.uploader_sc_user_id = $2
  AND (t.primary_artist_id = $1
    OR EXISTS (SELECT 1
               FROM track_artists ta
               WHERE ta.track_id = t.id
                 AND ta.artist_id = $1))
