-- SmartWave refactor: user_taste EMA pipeline is gone.
-- user_events колонки `taste_applied_at` и `seeded` существовали только для
-- этого пайплайна: первая помечала события, для которых уже применили EMA-апдейт
-- в qdrant `user_taste_*`, вторая отличала backfill из /me/likes от реальных
-- кликов. Ничего этого больше нет, колонки никем не читаются.

DROP INDEX IF EXISTS "user_events_taste_applied_at_idx";--> statement-breakpoint
ALTER TABLE "user_events" DROP COLUMN IF EXISTS "taste_applied_at";--> statement-breakpoint
ALTER TABLE "user_events" DROP COLUMN IF EXISTS "seeded";
