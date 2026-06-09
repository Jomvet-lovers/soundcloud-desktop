SELECT urn
FROM playlists
WHERE owner_sc_user_id = $1
  AND desired_rev > synced_rev
