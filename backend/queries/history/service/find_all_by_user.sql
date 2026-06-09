SELECT id,
       soundcloud_user_id,
       sc_track_id,
       title,
       artist_name,
       artist_urn,
       artwork_url,
       duration,
       played_at
FROM listening_history
WHERE soundcloud_user_id = ANY ($1)
ORDER BY played_at DESC LIMIT $2
OFFSET $3
