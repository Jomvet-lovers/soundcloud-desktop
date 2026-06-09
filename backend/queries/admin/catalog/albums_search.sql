-- AlbumListRow field order
SELECT al.id,
       al.title,
       al.type                                                               AS "type_!",
       al.release_year,
       al.primary_artist_id,
       a.name                                                                AS "primary_artist_name?",
       (SELECT COUNT(*) ::int8 FROM album_tracks t WHERE t.album_id = al.id) AS "track_count!"
FROM albums al
         LEFT JOIN artists a ON a.id = al.primary_artist_id
WHERE ($1::text IS NULL OR al.title ILIKE $1)
ORDER BY al.title ASC LIMIT $2
