-- Реэнкюив owned-плейлистов с pending desired_rev > synced_rev.
INSERT INTO sync_queue (user_id, action_type, target_urn)
SELECT p.owner_sc_user_id, 'playlist_sync', p.urn
FROM playlists p
WHERE p.desired_rev > p.synced_rev
  AND p.owner_sc_user_id IS NOT NULL
  AND NOT EXISTS (SELECT 1
                  FROM sync_queue q
                  WHERE q.user_id = p.owner_sc_user_id
                    AND q.dead = false
                    AND q.target_urn = p.urn
                    AND q.action_type = 'playlist_sync')
    LIMIT 500
ON CONFLICT (user_id, action_type, target_urn) DO
UPDATE SET
    next_run_at = now(), locked_at = NULL, dead = false,
    retry_count = 0, last_error = NULL
