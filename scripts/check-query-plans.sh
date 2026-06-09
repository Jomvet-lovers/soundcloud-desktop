#!/usr/bin/env bash
# «Светит» Seq Scan по большим/горячим таблицам в планах запросов backend/queries/*.sql.
# EXPLAIN (GENERIC_PLAN) — PG16+, планирует параметризованный запрос БЕЗ значений ($1..).
# Осмысленно ТОЛЬКО против БД с прод-подобной статистикой — иначе пустая БД планирует
# всё как Seq Scan. Прогрей: psql "$DATABASE_URL" -f scripts/load-approx-stats.sql
# Требует: DATABASE_URL, psql, jq. Advisory; FAIL_ON_SEQSCAN=1 — валит билд.
set -uo pipefail
: "${DATABASE_URL:?DATABASE_URL required}"
QDIR="${QUERIES_DIR:-backend/queries}"
BIG="${BIG_TABLES:-tracks track_artists user_events albums album_tracks sc_track_counters artists wanted_tracks user_likes_tracks playlist_tracks listening_history}"

cd "$(git rev-parse --show-toplevel)"
fail=0; checked=0
while IFS= read -r f; do
  sql=$(cat "$f")
  plan=$(psql "$DATABASE_URL" -tAqX -c "EXPLAIN (GENERIC_PLAN, FORMAT JSON) $sql" 2>/dev/null) || continue
  [ -z "$plan" ] && continue
  checked=$((checked + 1))
  hits=$(printf '%s' "$plan" | jq -r '[.. | objects | select(."Node Type"=="Seq Scan") | ."Relation Name"] | unique[]?' 2>/dev/null)
  for t in $hits; do
    for b in $BIG; do
      [ "$t" = "$b" ] && { printf '  ⚠ Seq Scan on %-20s %s\n' "$t" "${f#"$QDIR"/}"; fail=1; }
    done
  done
done < <(find "$QDIR" -name '*.sql' | sort)

echo "проверено $checked .sql"
if [ "$fail" = 1 ]; then
  echo "↑ потенциально медленные планы (нет индекса / запрос не подхватывает существующий). Сверь EXPLAIN (ANALYZE, BUFFERS) на проде."
  [ "${FAIL_ON_SEQSCAN:-0}" = 1 ] && exit 1
fi
exit 0
