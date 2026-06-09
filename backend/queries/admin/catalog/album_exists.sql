SELECT EXISTS(SELECT 1 FROM albums WHERE id = $1) AS "exists!"
