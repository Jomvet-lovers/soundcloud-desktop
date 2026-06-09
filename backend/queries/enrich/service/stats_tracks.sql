SELECT COUNT(*) FILTER (WHERE enrich_state = 'pending')::int8 AS "pending!", COUNT(*) FILTER (WHERE enrich_state = 'done')::int8 AS "done!", COUNT(*) FILTER (WHERE enrich_state = 'failed')::int8 AS "failed!", COUNT(*) FILTER (WHERE enrich_state = 'dead')::int8 AS "dead!", COUNT(*) FILTER (WHERE enrich_locked_at IS NOT NULL)::int8 AS "in_flight!"
FROM tracks
