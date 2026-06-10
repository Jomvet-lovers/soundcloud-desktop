DELETE
FROM album_artists aa USING album_artists h
WHERE aa.artist_id = $1
  AND h.artist_id = $2
  AND h.album_id = aa.album_id
  AND h.role = aa.role
