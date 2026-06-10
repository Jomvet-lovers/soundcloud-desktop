WITH ul AS (SELECT regexp_replace(user_id, '^soundcloud:users:', '') AS uid,
                   sc_track_id,
                   row_number() OVER (PARTITION BY regexp_replace(user_id, '^soundcloud:users:', '')
                       ORDER BY created_at DESC)                     AS rn
            FROM user_likes_tracks
            WHERE wanted_state = true),
     ua AS (SELECT DISTINCT ul.uid, ta.artist_id
            FROM ul
                     JOIN tracks t ON t.sc_track_id = ul.sc_track_id
                     JOIN track_artists ta ON ta.track_id = t.id AND ta.role = 'primary'
                     JOIN artists ar ON ar.id = ta.artist_id AND ar.merged_into IS NULL
            WHERE ul.rn <= 500),
     cnt AS (SELECT artist_id, count(*) AS likers
             FROM ua
             GROUP BY artist_id),
     pairs AS (SELECT x.artist_id AS a,
                      y.artist_id AS b,
                      count(*)    AS co
               FROM ua x
                        JOIN ua y ON x.uid = y.uid AND x.artist_id < y.artist_id
               GROUP BY 1, 2
               HAVING count(*) >= 2),
     weighted AS (SELECT p.a,
                         p.b,
                         p.co,
                         (p.co / (sqrt(ca.likers::float8 * cb.likers::float8) + $1))::real AS w
                  FROM pairs p
                           JOIN cnt ca ON ca.artist_id = p.a
                           JOIN cnt cb ON cb.artist_id = p.b),
     ranked AS (SELECT a,
                       b,
                       co,
                       w,
                       row_number() OVER (PARTITION BY a ORDER BY w DESC) AS ra,
                       row_number() OVER (PARTITION BY b ORDER BY w DESC) AS rb
                FROM weighted)
INSERT
INTO artist_colike (a_id, b_id, co, w, updated_at)
SELECT a, b, co::int, w, now()
FROM ranked
WHERE ra <= $2
   OR rb <= $2
ON CONFLICT (a_id, b_id) DO UPDATE
    SET co         = EXCLUDED.co,
        w          = EXCLUDED.w,
        updated_at = EXCLUDED.updated_at
