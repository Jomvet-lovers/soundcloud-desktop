WITH disl AS (SELECT ta.artist_id, COUNT(*) AS dc
              FROM disliked_tracks dt
                       JOIN tracks it ON it.sc_track_id = dt.sc_track_id
                       JOIN track_artists ta ON ta.track_id = it.id AND ta.role = 'primary'
              WHERE dt.sc_user_id = ANY ($1)
              GROUP BY ta.artist_id),
     lik AS (SELECT ta.artist_id, COUNT(*) AS lc
             FROM user_likes_tracks ul
                      JOIN tracks it ON it.sc_track_id = ul.sc_track_id
                      JOIN track_artists ta ON ta.track_id = it.id AND ta.role = 'primary'
             WHERE ul.user_id = ANY ($1)
               AND ul.wanted_state = true
             GROUP BY ta.artist_id)
SELECT d.artist_id AS "artist_id!"
FROM disl d
         LEFT JOIN lik l ON l.artist_id = d.artist_id
WHERE d.dc >= $2
  AND d.dc > COALESCE(l.lc, 0)
