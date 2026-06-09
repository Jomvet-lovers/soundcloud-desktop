SELECT aa.artist_id AS "artist_id!", 0.8::real AS "w!"
FROM tracks it
         JOIN album_artists aa ON aa.album_id = it.album_id
         JOIN artists a ON a.id = aa.artist_id
WHERE it.sc_track_id = $1
  AND a.merged_into IS NULL
