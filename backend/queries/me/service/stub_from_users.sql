SELECT jsonb_build_object(
               'id', $2::int8, 'urn', urn, 'username', username, 'full_name', full_name,
               'avatar_url', COALESCE(avatar_url, ''), 'permalink_url', COALESCE(permalink_url, ''),
               'followers_count', COALESCE(followers_count, 0),
               'followings_count', COALESCE(followings_count, 0),
               'track_count', COALESCE(tracks_count, 0),
               'playlist_count', COALESCE(playlists_count, 0),
               'public_favorites_count', 0
       ) AS "profile!"
FROM users
WHERE sc_user_id = $1
