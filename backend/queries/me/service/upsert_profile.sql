INSERT INTO user_profiles (soundcloud_user_id, profile_json, synced_at)
VALUES ($1, $2, now()) ON CONFLICT (soundcloud_user_id)
DO
UPDATE SET profile_json = EXCLUDED.profile_json, synced_at = now()
