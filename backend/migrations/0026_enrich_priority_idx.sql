-- enrich pickup отдаёт приоритет user-relevant трекам через index_priority
-- (Like=1, Playlist=2, Discovery=5) — лайки/owned линкуются к артистам раньше
-- discovery-фаерхоуза. Индекс под новый ORDER BY (index_priority, enriched_at).
CREATE INDEX IF NOT EXISTS tracks_enrich_pickup_pri_idx
    ON tracks (index_priority, enriched_at NULLS FIRST, enrich_attempts)
    WHERE enrich_state IN ('pending', 'failed');
