SELECT EXISTS(SELECT 1 FROM artists WHERE normalized_name = $1 AND merged_into IS NULL) AS "exists!"
