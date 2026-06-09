SELECT aura_id, custom_hex
FROM user_auras
WHERE user_urn = ANY ($1)
