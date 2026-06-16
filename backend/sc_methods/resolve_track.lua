-- sc.resolve_track — resolve a SoundCloud permalink URL to apiv2 track metadata.
--
-- inputs:  { url = "https://soundcloud.com/artist/track" }
-- output:  { ok = true, track = <apiv2 track> } | { ok = false, reason = "not_a_track" | "gone" }
--
-- Failure convention: error() -> the relay retries on the next client; return {...} ->
-- terminal (back to the backend).

local cid = client_id()
if cid == nil or cid == "" then
  error("no client_id")
end

local url = "https://api-v2.soundcloud.com/resolve?url=" .. urlencode(inputs.url) .. "&client_id=" .. cid
local resp = http({ url = url, method = "GET" })
local s = resp.status

if s == 200 then
  local data = json_decode(resp.body)
  if type(data) ~= "table" or data.kind ~= "track" then
    return { ok = false, reason = "not_a_track" }
  end
  return { ok = true, track = data }
elseif s == 404 then
  return { ok = false, reason = "gone" }
else
  error("resolve status " .. tostring(s))
end
