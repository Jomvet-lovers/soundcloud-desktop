SELECT sc_track_id
FROM tracks
WHERE cover_of_artist_id = $1
  AND upload_kind = 'cover'
ORDER BY COALESCE(play_count_sc, 0) DESC, sc_synced_at DESC LIMIT $2
OFFSET $3
