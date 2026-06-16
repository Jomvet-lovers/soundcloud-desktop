-- sc.track_by_id — apiv2 /tracks/{id} via the relay.
-- Recovers the real full_duration when apiv1 returned the 30000ms preview sentinel.
--
-- inputs:  { id = "123456" }
-- output:  { ok = true, track = <apiv2 track, RAW> } | { ok = false, reason = "no_track" | "gone" }

local cid = client_id()
if cid == nil or cid == "" then
  error("no client_id")
end

local url = "https://api-v2.soundcloud.com/tracks/"
  .. urlencode(tostring(inputs.id))
  .. "?client_id=" .. urlencode(cid)
local resp = http({ url = url, method = "GET" })
local s = resp.status

if s == 200 then
  local data = json_decode(resp.body)
  if type(data) ~= "table" or data.id == nil then
    return { ok = false, reason = "no_track" }
  end
  return { ok = true, track = data }
elseif s == 404 then
  return { ok = false, reason = "gone" }
else
  error("track_by_id status " .. tostring(s))
end
