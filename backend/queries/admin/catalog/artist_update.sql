-- ArtistRow field order; COALESCE keeps Option params nullable for sqlx infer
UPDATE artists
SET name            = COALESCE($2, name),
    normalized_name = COALESCE($3, normalized_name),
    country         = COALESCE($4, country),
    bio             = COALESCE($5, bio),
    avatar_url      = COALESCE($6, avatar_url),
    sc_user_id      = COALESCE($7, sc_user_id),
    confidence      = COALESCE($8, confidence),
    updated_at      = now()
WHERE id = $1 RETURNING id, name, normalized_name, country, avatar_url, bio, sc_user_id, source,
          confidence, mb_artist_id, spotify_artist_id, genius_artist_id, merged_into, created_at, updated_at
