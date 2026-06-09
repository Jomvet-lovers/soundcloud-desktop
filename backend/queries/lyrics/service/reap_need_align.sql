-- plain-but-unsynced tracks past age → re-queue align; stale pending only
SELECT lc.sc_track_id
FROM lyrics_cache lc
         JOIN tracks t ON t.sc_track_id = lc.sc_track_id
WHERE lc.plain_text IS NOT NULL
  AND length(lc.plain_text) > 0
  AND lc.synced_lrc IS NULL
  AND lc.created_at < $1
  AND (t.transcribe_state IS NULL OR (t.transcribe_state = 'pending' AND t.transcribe_at < $3))
ORDER BY lc.created_at ASC LIMIT $2
