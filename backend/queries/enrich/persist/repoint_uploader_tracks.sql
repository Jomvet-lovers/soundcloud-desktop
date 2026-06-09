UPDATE tracks
SET enrich_state       = 'pending',
    enrich_next_run_at = now() + (random() * interval '15 minutes')
WHERE uploader_sc_user_id = $1
  AND enrich_locked_at IS NULL
  AND enrich_state IN ('done', 'failed')
  AND (primary_artist_id IS NULL OR primary_artist_id <> $2)
