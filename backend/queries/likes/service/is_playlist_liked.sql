SELECT EXISTS(SELECT 1
              FROM user_likes_playlists
              WHERE user_id = ANY ($1)
                AND playlist_urn = $2
                AND wanted_state = true) AS "exists!"
