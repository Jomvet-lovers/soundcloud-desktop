INSERT INTO user_owned_playlists (user_id, playlist_urn, progress, synced_at)
VALUES ($1, $2, false, now()) ON CONFLICT (user_id, playlist_urn) DO
UPDATE SET synced_at = now()
