-- Global playlist search. Columns in PlaylistRow order.
SELECT urn,
       sc_playlist_id,
       title,
       title_normalized,
       description,
       genre,
       tags,
       artwork_url,
       permalink_url,
       owner_sc_user_id,
       owner_urn,
       owner_username,
       track_count,
       duration_ms,
       playlist_type,
       kind,
       sharing,
       release_year,
       release_date,
       label_name,
       likes_count_sc,
       reposts_count_sc,
       sc_created_at,
       sc_last_modified,
       tracks_synced_at,
       sc_synced_at,
       last_read_at,
       created_at,
       updated_at,
       desired_rev,
       synced_rev
FROM playlists
WHERE sharing = 'public'
  AND (
    title_normalized LIKE $1
        OR LOWER(title) LIKE $1
        OR LOWER(owner_username) LIKE $1
    )
ORDER BY likes_count_sc DESC NULLS LAST, sc_synced_at DESC, urn DESC LIMIT $2
OFFSET $3
