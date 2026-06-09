UPDATE user_likes_playlists
SET progress  = false,
    synced_at = now()
WHERE user_id = $1
  AND playlist_urn = $2
  AND wanted_state = true
