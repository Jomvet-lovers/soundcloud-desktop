DELETE
FROM sessions
WHERE id IN (SELECT id
             FROM (SELECT id,
                          row_number() OVER (
            PARTITION BY soundcloud_user_id ORDER BY updated_at DESC
        ) AS rn
                   FROM sessions
                   WHERE soundcloud_user_id IS NOT NULL) t
             WHERE t.rn > 1)
  AND expires_at <= now()
