-- Self-gen (whisper) lifecycle на уровне трека, отдельно от наличия лирики.
--
-- lyrics_cache остаётся ответом на вопрос «нашли ли мы текст вообще»
-- (агрегаторы lrclib/mxm/genius/netease + self_gen). transcribe_state — это
-- ортогональный вопрос «гоняли ли мы whisper по этому треку и нужно ли ещё».
--
--   NULL       — ни разу не пробовали (eligible);
--   'pending'  — джоб в очереди / в работе. Это атомарный клейм: защищает от
--                двойного enqueue из upload-события и параллельного
--                user-запроса (fork B), а также от двух воркеров;
--   'done'     — whisper отработал, результат сохранён, больше не берём;
--   'disabled' — self-gen-disable: whisper не дал речи (инструментал/шум) либо
--                стабильно падает. Трек больше НЕ транскрайбится, но агрегаторы
--                продолжают пытаться подтянуть текст по user-запросу/реапу.
--
-- transcribe_at — момент последней смены state. По нему реапятся «зависшие»
-- 'pending' (воркер умер / max_deliver исчерпан / бэк упал между клеймом и
-- publish). До этой миграции пустой результат whisper НЕ писал ничего, а
-- reap_whisper.need_full выбирает треки без строки в lyrics_cache — поэтому
-- инструменталы перекачивались + demucs + whisper на каждом цикле вечно.
ALTER TABLE tracks ADD COLUMN transcribe_state varchar(16);
ALTER TABLE tracks ADD COLUMN transcribe_at timestamptz;

-- Recovery «зависших» pending — маленький партиал-индекс (pending — редкое
-- транзиентное состояние, индекс компактный).
CREATE INDEX tracks_transcribe_pending_idx ON tracks (transcribe_at)
    WHERE transcribe_state = 'pending';
