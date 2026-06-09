SELECT id,
       name,
       normalized_name,
       country,
       avatar_url,
       confidence,
       track_count_primary,
       track_count_featured,
       album_count_denorm,
       monthly_listeners,
       trending_score,
       popularity_score,
       tags,
       is_star,
       star_aura_id,
       star_custom_hex
FROM artists
WHERE id = ANY ($1)
  AND merged_into IS NULL
