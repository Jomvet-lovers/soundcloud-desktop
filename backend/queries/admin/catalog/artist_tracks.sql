-- TrackListRow field order. Треки, у которых ЭТОТ артист в составе (любая роль).
-- EXISTS, а не JOIN — иначе дубли строк при нескольких кредитах на одного артиста.
SELECT t.id,
       t.sc_track_id,
       t.title,
       t.metadata_artist,
       t.artwork_url,
       t.primary_artist_id,
       a.name   AS "primary_artist_name?",
       t.album_id,
       al.title AS "album_title?",
       t.enrich_state,
       t.release_year
FROM tracks t
         LEFT JOIN artists a ON a.id = t.primary_artist_id
         LEFT JOIN albums al ON al.id = t.album_id
WHERE EXISTS (SELECT 1
              FROM track_artists ta
              WHERE ta.track_id = t.id
                AND ta.artist_id = $1)
ORDER BY t.sc_created_at DESC NULLS LAST
LIMIT $2
