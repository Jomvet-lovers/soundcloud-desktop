SELECT EXISTS(SELECT 1 FROM tracks WHERE id = $1) AS "exists!"
