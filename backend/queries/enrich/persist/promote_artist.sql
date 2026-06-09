UPDATE artists
SET mb_artist_id     = COALESCE(mb_artist_id, $2),
    genius_artist_id = COALESCE(genius_artist_id, $3),
    sc_user_id       = COALESCE(sc_user_id, $4),
    source           = CASE WHEN $5 THEN $6 ELSE source END,
    confidence       = CASE WHEN $5 THEN $7 ELSE confidence END,
    updated_at       = now()
WHERE id = $1
