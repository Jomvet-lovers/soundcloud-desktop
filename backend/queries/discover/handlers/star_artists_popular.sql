-- Star spotlight, popular strategy: top by popularity/listeners.
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
WHERE merged_into IS NULL
  AND is_star = TRUE
  AND (track_count_primary > 0 OR track_count_featured > 0)
  AND id <> ALL ($1)
ORDER BY popularity_score DESC, monthly_listeners DESC, normalized_name LIMIT $2
