SELECT a.id, a.name, aa.role, a.avatar_url
FROM album_artists aa
         JOIN artists a ON a.id = aa.artist_id
WHERE aa.album_id = $1
ORDER BY CASE aa.role WHEN 'primary' THEN 0 WHEN 'featured' THEN 1 ELSE 2 END, a.name
