-- ArtistRow field order
SELECT id,
       name,
       normalized_name,
       country,
       avatar_url,
       bio,
       sc_user_id,
       source,
       confidence,
       mb_artist_id,
       spotify_artist_id,
       genius_artist_id,
       merged_into,
       created_at,
       updated_at
FROM artists
WHERE id = $1
