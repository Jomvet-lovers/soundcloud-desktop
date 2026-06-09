SELECT ta.artist_id AS "artist_id!",
       (CASE ta.role WHEN 'primary' THEN 1.0 WHEN 'featured' THEN 0.6 ELSE 0.5 END) ::real AS "w!"
FROM tracks it
         JOIN track_artists ta ON ta.track_id = it.id AND ta.role IN ('primary', 'featured', 'remixer')
         JOIN artists a ON a.id = ta.artist_id
WHERE it.sc_track_id = $1
  AND a.merged_into IS NULL
