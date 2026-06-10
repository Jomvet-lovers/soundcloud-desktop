-- Триаж-админка: дефолтный листинг сортирует tracks по sc_created_at (7M+
-- строк без индекса = seq scan + sort на каждое открытие), поиск идёт по
-- lower(title)/lower(metadata_artist) по ВСЕМ sharing (партиальные
-- public-индексы поиска не подходят).
-- На проде индексы предсоздаются вручную через CREATE INDEX CONCURRENTLY —
-- здесь IF NOT EXISTS делает no-op.
CREATE INDEX IF NOT EXISTS tracks_sc_created_at_desc_idx
    ON tracks (sc_created_at DESC NULLS LAST);
CREATE INDEX IF NOT EXISTS tracks_admin_title_lower_trgm
    ON tracks USING gin (lower(title) gin_trgm_ops);
CREATE INDEX IF NOT EXISTS tracks_admin_meta_lower_trgm
    ON tracks USING gin (lower(metadata_artist) gin_trgm_ops);
