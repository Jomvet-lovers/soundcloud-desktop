SELECT t.sc_track_id
FROM track_artists ta
         JOIN tracks t ON t.id = ta.track_id
         LEFT JOIN sc_track_counters c ON c.sc_track_id = t.sc_track_id
WHERE ta.artist_id = $1
  AND COALESCE(t.upload_kind, '') NOT IN ('cover', 'reupload')
  AND t.cover_of_artist_id IS NULL
  AND (t.uploader_sc_user_id IS NULL
       OR EXISTS (SELECT 1
                  FROM artist_sc_accounts asa
                  WHERE asa.artist_id = ta.artist_id
                    AND asa.sc_user_id = t.uploader_sc_user_id
                    AND asa.source <> 'reupload_pattern')
       OR NOT EXISTS (SELECT 1
                      FROM artist_sc_accounts a2
                      WHERE a2.artist_id = ta.artist_id
                        AND a2.source <> 'reupload_pattern'))
ORDER BY t.release_date DESC NULLS LAST, t.sc_created_at DESC NULLS LAST, t.release_year DESC NULLS LAST, t.created_at DESC, t.id DESC LIMIT $2
OFFSET $3
