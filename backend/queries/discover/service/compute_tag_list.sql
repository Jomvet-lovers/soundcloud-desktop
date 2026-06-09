SELECT g AS "tag!", COUNT(*) ::bigint AS "n!"
FROM (SELECT UNNEST(tags) AS g
      FROM artists
      WHERE merged_into IS NULL
        AND array_length(tags, 1) > 0) t
WHERE TRIM(g) <> ''
GROUP BY g
ORDER BY COUNT(*) DESC, g LIMIT $1
