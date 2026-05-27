-- `cdn_tracks.quality` исходно создавалась как varchar(4) под легаси-значения
-- 'hq' / 'sq' / 'lq'. После m4a-перехода streaming пишет туда 'single'
-- (6 символов) — все INSERT'ы падают с `value too long for type character
-- varying(4)`, поэтому новые CDN-записи в БД не сохраняются.
--
-- Расширяем колонку. Без data-loss: старые 'hq'/'sq'/'lq' остаются валидны,
-- unique-индекс (track_urn, quality) переживает расширение типа.

ALTER TABLE cdn_tracks ALTER COLUMN quality TYPE varchar(16);
