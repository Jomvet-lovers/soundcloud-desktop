WITH active AS (SELECT DISTINCT regexp_replace(soundcloud_user_id, '^soundcloud:users:', '') AS uid
                FROM sessions
                WHERE updated_at > now() - interval '14 days'),
     seed AS (SELECT x.uid,
                     ta.artist_id,
                     row_number() OVER (PARTITION BY x.uid ORDER BY count(*) DESC) AS rn
              FROM active x
                       JOIN user_likes_tracks ul
                            ON regexp_replace(ul.user_id, '^soundcloud:users:', '') = x.uid
                                AND ul.wanted_state = true
                       JOIN tracks t ON t.sc_track_id = ul.sc_track_id
                       JOIN track_artists ta ON ta.track_id = t.id AND ta.role = 'primary'
              WHERE ul.created_at > now() - interval '180 days'
              GROUP BY x.uid, ta.artist_id),
     hood AS (SELECT DISTINCT artist_id
              FROM seed
              WHERE rn <= 12
              UNION
              SELECT CASE WHEN e.a_id = s.artist_id THEN e.b_id ELSE e.a_id END
              FROM (SELECT DISTINCT artist_id FROM seed WHERE rn <= 6) s
                       JOIN LATERAL (SELECT a_id, b_id
                                     FROM artist_colike
                                     WHERE a_id = s.artist_id
                                        OR b_id = s.artist_id
                                     ORDER BY w DESC
                                     LIMIT 8) e ON true),
     cand AS (SELECT it.id,
                     row_number() OVER (PARTITION BY ta.artist_id ORDER BY COALESCE(c.play_count, 0) DESC) AS rn
              FROM hood h
                       JOIN track_artists ta ON ta.artist_id = h.artist_id AND ta.role = 'primary'
                       JOIN tracks it ON it.id = ta.track_id
                       LEFT JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
              WHERE it.sharing = 'public'
                AND it.storage_state = 'pending'
                AND (it.storage_priority > 0 OR it.index_priority > 0))
UPDATE tracks
SET storage_priority = 0,
    index_priority   = 0
WHERE id IN (SELECT id FROM cand WHERE rn <= 4)
