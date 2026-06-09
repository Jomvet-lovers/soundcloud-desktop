SELECT id,
       entity_type,
       entity_id,
       position,
       active,
       note,
       created_at,
       updated_at
FROM discover_promoted
ORDER BY active DESC, position ASC, created_at ASC
