UPDATE tracks
SET index_state    = 'indexed',
    indexed_at     = now(),
    index_attempts = 0,
    updated_at     = now()
WHERE sc_track_id = $1
  AND index_state <> 'indexed'
