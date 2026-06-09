-- LEFT JOIN -> a.name/a.avatar_url forced nullable to match Option fields.
SELECT al.id,
       al.title,
       al.normalized_title,
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
WHERE al.id = ANY ($1)
