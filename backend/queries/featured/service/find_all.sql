SELECT id, "type" AS "item_type!", sc_urn, weight, active, created_at
FROM featured_items
ORDER BY created_at DESC
