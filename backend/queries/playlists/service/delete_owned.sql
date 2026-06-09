DELETE
FROM user_owned_playlists
WHERE user_id = ANY ($1)
  AND playlist_urn = $2
