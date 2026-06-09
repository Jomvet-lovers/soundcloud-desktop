-- TrackListRow field order
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
WHERE ($1::text IS NULL OR t.title ILIKE $1 OR t.metadata_artist ILIKE $1 OR t.sc_track_id = $2)
ORDER BY t.sc_created_at DESC NULLS LAST LIMIT $3
