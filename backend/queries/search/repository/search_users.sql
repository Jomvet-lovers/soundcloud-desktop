-- User search via trgm. Columns in UserRow order.
SELECT sc_user_id,
       urn,
       username,
       username_normalized,
       full_name,
       first_name,
       last_name,
       permalink,
       permalink_url,
       avatar_url,
       country,
       city,
       description,
       verified,
       followers_count,
       followings_count,
       tracks_count,
       playlists_count,
       reposts_count,
       comments_count,
       kind,
       sc_created_at,
       sc_last_modified,
       sc_synced_at,
       last_read_at,
       created_at,
       updated_at
FROM users
WHERE username_normalized LIKE $1
   OR LOWER(username) LIKE $1
ORDER BY followers_count DESC NULLS LAST, sc_synced_at DESC, sc_user_id DESC LIMIT $2
OFFSET $3
