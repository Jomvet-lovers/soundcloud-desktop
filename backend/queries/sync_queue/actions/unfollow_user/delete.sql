DELETE
FROM user_followings
WHERE user_id = $1
  AND target_user_urn = $2
  AND wanted_state = false
