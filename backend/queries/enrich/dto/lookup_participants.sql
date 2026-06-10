SELECT ta.track_id   AS "track_id!",
       ta.role       AS "role!",
       ta.confidence AS "ta_confidence!",
       a.id          AS "artist_id!",
       a.name        AS "artist_name!",
       a.avatar_url  AS "artist_avatar_url",
       a.sc_user_id  AS "artist_sc_user_id",
       a.source      AS "artist_source!",
       a.confidence  AS "artist_confidence!"
FROM track_artists ta
         JOIN artists a ON a.id = ta.artist_id
WHERE ta.track_id = ANY ($1)
ORDER BY ta.track_id,
         CASE ta.role
             WHEN 'primary' THEN 0
             WHEN 'featured' THEN 1
             WHEN 'remixer' THEN 2
             WHEN 'producer' THEN 3
             WHEN 'composer' THEN 4
             WHEN 'vocal' THEN 5
             ELSE 6
             END,
         ta.position
