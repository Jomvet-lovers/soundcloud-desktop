UPDATE wanted_tracks
SET resolve_locked_at   = NULL,
    resolve_next_run_at = now()
        + LEAST(interval '7 days',
                interval '10 minutes' * power(2, LEAST(resolve_attempts, 10))),
    status              = CASE WHEN resolve_attempts >= 8 THEN 'unresolvable' ELSE status END
WHERE id = ANY ($1)
  AND status = 'wanted'
  AND track_id IS NULL
