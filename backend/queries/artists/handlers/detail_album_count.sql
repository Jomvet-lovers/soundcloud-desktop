SELECT COUNT(*) ::bigint AS "count!"
FROM (SELECT id
      FROM albums
      WHERE primary_artist_id = $1
      UNION
      SELECT album_id
      FROM album_artists
      WHERE artist_id = $1
      UNION
      SELECT wta.album_id
      FROM wanted_track_albums wta
               JOIN wanted_tracks wt ON wt.id = wta.wanted_track_id
      WHERE wt.primary_artist_id = $1) a
