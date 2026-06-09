DELETE
FROM user_likes_playlists
WHERE user_id = $1
  AND playlist_urn = $2
  AND wanted_state = false
