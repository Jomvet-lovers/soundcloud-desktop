-- no-transaction
-- FTS over lyrics_cache для /search/lyrics (mode=text) через EXPRESSION GIN index.
--
-- Намеренно НЕ добавляем GENERATED STORED колонку: ADD COLUMN ... GENERATED
-- переписывает всю таблицу под ACCESS EXCLUSIVE и блокирует живой lyrics-пайплайн
-- (сотни тыс. строк, горячий прод). Expression-индекс не трогает таблицу, а
-- CONCURRENTLY не держит долгий write-lock (потому `-- no-transaction` первой
-- строкой: CONCURRENTLY нельзя внутри транзакции, а миграции транзакционны).
--
-- regconfig 'simple' — без стемминга/стопслов, безопасно для смешанного корпуса
-- (RU/JA/KO/EN). LRC-таймстемпы вырезаем regexp_replace'ом, иначе цифры таймкодов
-- засоряют лексемы. Выражение IMMUTABLE (literal config + immutable regexp_replace),
-- поэтому годится для индекса.
--
-- ВАЖНО: запрос в `search::vibe::lyrics_text` обязан использовать ровно это же
-- выражение (LYRICS_FTS_EXPR), иначе планировщик не подхватит индекс.
--
-- IF NOT EXISTS делает повтор no-op. Упавший concurrent-build оставляет
-- INVALID-индекс, который IF NOT EXISTS молча подхватил бы по имени (миграция
-- «успешна», но планировщик его не использует → seq scan под 2.5s timeout). DO
-- ниже сносит ровно INVALID-остаток (валидный, в т.ч. пред-созданный руками на
-- проде, не трогает — plain DROP, не CONCURRENTLY: на невалидном индексе lock
-- мгновенный), после чего CREATE его перестроит.

DO
$$
BEGIN
    IF
EXISTS (
        SELECT 1 FROM pg_class c
        JOIN pg_index i ON i.indexrelid = c.oid
        WHERE c.relname = 'lyrics_cache_fts_gin' AND NOT i.indisvalid
    ) THEN
        EXECUTE 'DROP INDEX "lyrics_cache_fts_gin"';
END IF;
END $$;

CREATE INDEX CONCURRENTLY IF NOT EXISTS "lyrics_cache_fts_gin"
    ON "lyrics_cache" USING GIN (
    to_tsvector(
    'simple',
    coalesce ("plain_text", '')
    || ' '
    || regexp_replace(coalesce ("synced_lrc", ''), '\[[0-9:.]+\]', ' ', 'g')
    )
    );
