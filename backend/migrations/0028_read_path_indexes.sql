-- listening_history: пагинация find_all (ORDER BY played_at DESC по юзеру) и
-- dedup-проба record() (user + 60s-окно). Раньше был только (soundcloud_user_id).
CREATE INDEX IF NOT EXISTS listening_history_user_played_idx
    ON listening_history (soundcloud_user_id, played_at DESC);

-- quality backfill pickup: только indexed-но-неоценённые (≈мало строк → компактный).
CREATE INDEX IF NOT EXISTS tracks_quality_pending_idx
    ON tracks (indexed_at DESC)
    WHERE quality_score IS NULL AND indexed_at IS NOT NULL;
