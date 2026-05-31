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
-- Ровно один statement: CONCURRENTLY под `-- no-transaction` не может делить
-- simple-query сообщение с другой командой — вторая команда вернёт implicit-tx
-- и CONCURRENTLY будет отклонён. IF NOT EXISTS делает повтор no-op.
CREATE INDEX CONCURRENTLY IF NOT EXISTS "lyrics_cache_fts_gin"
    ON "lyrics_cache" USING GIN (
    to_tsvector(
    'simple',
    coalesce ("plain_text", '')
    || ' '
    || regexp_replace(coalesce ("synced_lrc", ''), '\[[0-9:.]+\]', ' ', 'g')
    )
    );
