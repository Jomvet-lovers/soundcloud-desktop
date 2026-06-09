SELECT (SELECT COUNT(*) FROM user_likes_tracks WHERE user_id = ANY ($1) AND wanted_state = true) AS "likes!",
       (SELECT COALESCE(EXTRACT(EPOCH FROM MAX(created_at))::bigint, 0)
        FROM user_likes_tracks
        WHERE user_id = ANY ($1)
          AND wanted_state = true)                                                               AS "last_like!",
       (SELECT COUNT(*) FROM disliked_tracks WHERE sc_user_id = ANY ($1))                        AS "dislikes!"
