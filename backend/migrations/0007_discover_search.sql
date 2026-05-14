-- Trigram-индексы под подстрочный поиск в /discover/{artists,albums}.
-- ILIKE '%q%' без них = seq scan на млн+ строк.

CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE INDEX "artists_search_normalized_trgm"
    ON "artists" USING GIN ("normalized_name" gin_trgm_ops)
    WHERE merged_into IS NULL;

CREATE INDEX "artists_search_name_trgm"
    ON "artists" USING GIN (LOWER("name") gin_trgm_ops)
    WHERE merged_into IS NULL;

CREATE INDEX "albums_search_normalized_trgm"
    ON "albums" USING GIN ("normalized_title" gin_trgm_ops);

CREATE INDEX "albums_search_title_trgm"
    ON "albums" USING GIN (LOWER("title") gin_trgm_ops);
