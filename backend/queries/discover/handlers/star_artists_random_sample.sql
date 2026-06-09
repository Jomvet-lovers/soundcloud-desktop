-- Star spotlight, random strategy: BERNOULLI sample (primary path).
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
FROM artists TABLESAMPLE BERNOULLI(5)
WHERE merged_into IS NULL
  AND is_star = TRUE
  AND (track_count_primary
    > 0
   OR track_count_featured
    > 0)
  AND id <> ALL ($1)
    LIMIT $2
