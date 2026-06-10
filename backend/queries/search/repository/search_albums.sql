-- Album search via trgm. Columns in AlbumSearchRow order.
-- primary_artist_name/avatar come from LEFT JOIN -> force nullable (?).
SELECT al.id,
       al.title,
       al.type      AS kind,
       al.release_year,
       al.release_date,
       al.cover_url,
       al.confidence,
       al.track_count,
       al.total_duration_ms,
       al.popularity_score,
       al.is_star_artist,
       al.primary_artist_id,
       a.name       AS "primary_artist_name?",
       a.avatar_url AS "primary_artist_avatar?"
FROM albums al
         LEFT JOIN artists a ON a.id = al.primary_artist_id AND a.merged_into IS NULL
WHERE al.track_count > 0
  AND (
    al.normalized_title LIKE $4
        OR LOWER(al.title) LIKE $1
        OR LOWER(COALESCE(a.name, '')) LIKE $1
    )
ORDER BY al.popularity_score DESC, al.normalized_title ASC, al.id ASC LIMIT $2
OFFSET $3
