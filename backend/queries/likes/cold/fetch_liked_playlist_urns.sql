SELECT playlist_urn
FROM user_likes_playlists
WHERE user_id = ANY ($1)
  AND wanted_state = true
  AND playlist_urn = ANY ($2)
