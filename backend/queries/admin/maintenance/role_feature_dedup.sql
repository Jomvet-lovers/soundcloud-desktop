-- Пара уже есть с каноничной ролью — легаси-строку убираем.
DELETE
FROM track_artists ta USING track_artists h
WHERE ta.role = 'feature'
  AND h.track_id = ta.track_id
  AND h.artist_id = ta.artist_id
  AND h.role = 'featured'
