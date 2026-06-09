-- ArtistListRow field order
SELECT a.id,
       a.name,
       a.country,
       a.avatar_url,
       a.confidence,
       a.sc_user_id,
       a.source,
       (SELECT COUNT(*) ::int8 FROM track_artists ta WHERE ta.artist_id = a.id)    AS "track_count!",
       (SELECT COUNT(*) ::int8 FROM artist_sc_accounts s WHERE s.artist_id = a.id) AS "sc_accounts_count!"
FROM artists a
WHERE a.merged_into IS NULL
  AND ($1::text IS NULL OR a.name ILIKE $1 OR a.sc_user_id = $2)
ORDER BY a.confidence DESC, a.name ASC LIMIT $3
