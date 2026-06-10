-- Словарь для сегментации склеек: только доверенные артисты (внешний источник
-- или высокий confidence) — heuristic-мусор ("Intro", "Demo") режет настоящие
-- имена на куски.
SELECT normalized_name
FROM artists
WHERE normalized_name = ANY ($1)
  AND merged_into IS NULL
  AND (source IN ('genius', 'mb', 'isrc', 'sc_verified') OR confidence >= 0.7)
