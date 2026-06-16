-- Каверы/реапы «на» этого артиста: то, что мы НЕ показываем во вкладке треков.
-- Зеркало исключения из tracks_*_*.sql:
--   1. явный cover (resolver выставил cover_of_artist_id + upload_kind='cover');
--   2. трек, атрибутированный артисту как primary, но: помечен cover/reupload,
--      ЛИБО залит не-родным аккаунтом (uploader известен, не входит в «родные»
--      аккаунты артиста — source <> 'reupload_pattern' — и при этом у артиста
--      родные аккаунты вообще есть, иначе различить нельзя и трек считаем его).
SELECT t.sc_track_id
FROM tracks t
WHERE (t.cover_of_artist_id = $1 AND COALESCE(t.upload_kind, '') = 'cover')
   OR EXISTS (SELECT 1
              FROM track_artists ta
              WHERE ta.track_id = t.id
                AND ta.artist_id = $1
                AND ta.role = 'primary'
                AND (COALESCE(t.upload_kind, '') IN ('cover', 'reupload')
                     OR (t.uploader_sc_user_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1
                                         FROM artist_sc_accounts asa
                                         WHERE asa.artist_id = $1
                                           AND asa.sc_user_id = t.uploader_sc_user_id
                                           AND asa.source <> 'reupload_pattern')
                         AND EXISTS (SELECT 1
                                     FROM artist_sc_accounts a2
                                     WHERE a2.artist_id = $1
                                       AND a2.source <> 'reupload_pattern'))))
ORDER BY COALESCE(t.play_count_sc, 0) DESC, t.sc_synced_at DESC LIMIT $2
OFFSET $3
