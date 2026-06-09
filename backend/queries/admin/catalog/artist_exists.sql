SELECT EXISTS(SELECT 1 FROM artists WHERE id = $1) AS "exists!"
