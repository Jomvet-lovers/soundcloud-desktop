INSERT INTO featured_items ("type", sc_urn, weight, active)
VALUES ($1, $2, $3, $4) RETURNING id, "type" AS "item_type!", sc_urn, weight, active, created_at
