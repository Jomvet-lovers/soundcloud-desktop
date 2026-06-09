SELECT kind, url, source, verified
FROM artist_socials
WHERE artist_id = $1
ORDER BY kind, url
