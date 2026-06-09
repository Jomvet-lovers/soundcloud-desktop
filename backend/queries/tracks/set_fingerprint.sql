UPDATE tracks
SET audio_fingerprint = $2,
    updated_at        = now()
WHERE id = $1
