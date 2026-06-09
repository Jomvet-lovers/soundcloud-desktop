-- Прогрев планировщика прод-подобными размерами таблиц для EXPLAIN-гейта
-- (scripts/check-query-plans.sh) на БД БЕЗ реальных данных. Ставим только
-- pg_class.reltuples/relpages, чтобы планировщик выбирал индексы как на проде.
-- Магнитуды — грубо из прод pg_stat (2026-06). Обнови при сильном росте.
-- Требует superuser (dev/CI-контейнер postgres — ок).
DO
$$
DECLARE
r RECORD;
BEGIN
FOR r IN
SELECT *
FROM (VALUES ('tracks', 6600000),
             ('track_artists', 3400000),
             ('user_events', 130000000),
             ('albums', 900000),
             ('album_tracks', 1300000),
             ('sc_track_counters', 1800000),
             ('artists', 740000),
             ('wanted_tracks', 7000000),
             ('user_likes_tracks', 1400000),
             ('playlist_tracks', 600000),
             ('listening_history', 2000000)) AS t(name, n) LOOP
            EXECUTE format(
                'UPDATE pg_class SET reltuples = %s, relpages = GREATEST(%s / 100, 1) WHERE relname = %L AND relkind = ''r''',
                r.n, r.n, r.name);
END LOOP;
END
$$;
