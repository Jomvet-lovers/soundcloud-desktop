SELECT entity_type, entity_id
FROM discover_promoted
WHERE active = TRUE
ORDER BY position ASC, created_at ASC LIMIT $1
