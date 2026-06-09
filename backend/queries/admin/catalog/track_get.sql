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
WHERE t.id = $1
