WITH ranked AS (SELECT asa.artist_id,
                       ua.aura_id,
                       ua.custom_hex,
                       ROW_NUMBER() OVER (
               PARTITION BY asa.artist_id
               ORDER BY asa.verified DESC,
                        CASE asa.role WHEN 'main' THEN 0 WHEN 'demo' THEN 1 ELSE 2 END,
                        asa.sc_user_id
           ) AS rk
                FROM artist_sc_accounts asa
                         JOIN subscriptions s
                              ON s.user_urn = 'soundcloud:users:' || asa.sc_user_id
                         LEFT JOIN user_auras ua
                                   ON ua.user_urn = 'soundcloud:users:' || asa.sc_user_id
                WHERE asa.role IN ('main', 'demo')
                  AND s.exp_date > $1),
     star AS (SELECT artist_id, aura_id, custom_hex
              FROM ranked
              WHERE rk = 1),
     affected AS (SELECT artist_id
                  FROM star
                  UNION
                  SELECT id AS artist_id
                  FROM artists
                  WHERE is_star = TRUE
                    AND merged_into IS NULL)
UPDATE artists a
SET is_star         = (s.artist_id IS NOT NULL),
    star_aura_id    = s.aura_id,
    star_custom_hex = s.custom_hex FROM affected aff
LEFT JOIN star s
ON s.artist_id = aff.artist_id
WHERE a.id = aff.artist_id AND a.merged_into IS NULL
