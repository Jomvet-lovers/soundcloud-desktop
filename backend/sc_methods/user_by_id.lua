-- sc.user_by_id — apiv2 /users/{id} via the relay.
-- A token-free source for a user's public profile (avatar/username).
--
-- inputs:  { id = "183" }
-- output:  { ok = true, user = <apiv2 user, RAW> } | { ok = false, reason = "no_user" | "gone" }

local cid = client_id()
if cid == nil or cid == "" then
  error("no client_id")
end

local url = "https://api-v2.soundcloud.com/users/"
  .. urlencode(tostring(inputs.id))
  .. "?client_id=" .. urlencode(cid)
local resp = http({ url = url, method = "GET" })
local s = resp.status

if s == 200 then
  local data = json_decode(resp.body)
  if type(data) ~= "table" or data.id == nil then
    return { ok = false, reason = "no_user" }
  end
  return { ok = true, user = data }
elseif s == 404 then
  return { ok = false, reason = "gone" }
else
  error("user_by_id status " .. tostring(s))
end
