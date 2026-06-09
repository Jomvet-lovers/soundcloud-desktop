SELECT EXISTS (SELECT 1
               FROM user_owned_playlists
               WHERE user_id = ANY ($3)
                 AND playlist_urn = $2
               UNION ALL
               SELECT 1
               FROM playlists
               WHERE urn = $2
                 AND owner_sc_user_id = $1) AS "owns!"
