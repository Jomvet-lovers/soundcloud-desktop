SELECT al.id,
       al.title,
       al.type                                                                                   AS kind,
       al.release_year,
       al.cover_url,
       CASE WHEN al.primary_artist_id = $1 THEN 'primary' ELSE COALESCE(aa.role, 'featured') END AS "role!"
FROM albums al
         LEFT JOIN album_artists aa ON aa.album_id = al.id AND aa.artist_id = $1
WHERE al.primary_artist_id = $1
   OR aa.artist_id = $1
   OR al.id IN (SELECT wta.album_id
                FROM wanted_track_albums wta
                         JOIN wanted_tracks wt ON wt.id = wta.wanted_track_id
                WHERE wt.primary_artist_id = $1)
ORDER BY COALESCE(al.release_year, 0) DESC, al.title
