UPDATE tracks
SET canonical_track_id = $1,
    updated_at         = now()
WHERE id IN ($2, $3)
  AND (canonical_track_id IS NULL OR canonical_track_id <> $1)
