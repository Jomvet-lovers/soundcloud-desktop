SELECT profile_json, synced_at
FROM user_profiles
WHERE soundcloud_user_id = ANY ($1)
ORDER BY synced_at DESC LIMIT 1
