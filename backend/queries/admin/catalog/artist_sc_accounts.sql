-- ScAccountRow field order. LEFT JOIN users — кэш SC-профиля (ник/аватар/ссылка/
-- метрики), может отсутствовать. Два скалярных счётчика: сколько треков этого
-- аплоадера всего в каталоге и сколько из них залинкованы на ЭТОГО артиста.
SELECT s.sc_user_id,
       s.role,
       s.source,
       s.verified,
       s.notes,
       u.username                            AS "username?",
       u.avatar_url                          AS "avatar_url?",
       u.permalink_url                       AS "permalink_url?",
       COALESCE(u.verified, false)           AS "sc_verified!",
       u.followers_count                     AS "followers_count?",
       u.tracks_count                        AS "sc_tracks_count?",
       u.country                             AS "country?",
       (SELECT COUNT(*)
        FROM tracks t
        WHERE t.uploader_sc_user_id = s.sc_user_id)::int8
                                             AS "catalog_track_count!",
       (SELECT COUNT(*)
        FROM tracks t
        WHERE t.uploader_sc_user_id = s.sc_user_id
          AND EXISTS (SELECT 1
                      FROM track_artists ta
                      WHERE ta.track_id = t.id
                        AND ta.artist_id = $1))::int8
                                             AS "linked_track_count!"
FROM artist_sc_accounts s
         LEFT JOIN users u ON u.sc_user_id = s.sc_user_id
WHERE s.artist_id = $1
ORDER BY CASE s.role WHEN 'main' THEN 0 WHEN 'demo' THEN 1 ELSE 2 END,
         s.verified DESC,
         s.sc_user_id
