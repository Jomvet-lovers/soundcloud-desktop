-- sc.transcoding_resolve — turn an apiv2 transcoding media URL into a signed CDN URL,
-- using the relay's client_id (+ optional track_authorization JWT).
--
-- inputs:  { url = "<apiv2 .../media/.../stream/progressive>", track_authorization = "<jwt?>" }
-- output:  { ok = true, url = "<signed cdn url>", auth_token = "<licenseAuthToken?>" }
--          | { ok = false, reason = "no_url" | "gone" }
--
-- Failure convention: error() -> the relay retries on the next client.

local cid = client_id()
if cid == nil or cid == "" then
  error("no client_id")
end

local sep = "?"
if string.find(inputs.url, "?", 1, true) then
  sep = "&"
end
local target = inputs.url .. sep .. "client_id=" .. urlencode(cid)
if inputs.track_authorization ~= nil and inputs.track_authorization ~= "" then
  target = target .. "&track_authorization=" .. urlencode(inputs.track_authorization)
end

local resp = http({ url = target, method = "GET" })
local s = resp.status

if s == 200 then
  local data = json_decode(resp.body)
  if type(data) ~= "table" or data.url == nil then
    return { ok = false, reason = "no_url" }
  end
  return { ok = true, url = data.url, auth_token = data.licenseAuthToken }
elseif s == 404 then
  return { ok = false, reason = "gone", __verdict = "terminal" }
elseif s == 403 then
  -- The stream is forbidden from THIS region (geoblock at the stream-resolve step) —
  -- it may resolve from a client in an allowed country. Soft-negative, not a hard
  -- failure, so the relay polls other regions before giving up.
  return { ok = false, reason = "geoblocked", __verdict = "soft_negative" }
else
  -- 401 (dead client_id) / 429 / 5xx: transient/this-client → retry the next client.
  error("transcoding resolve status " .. tostring(s))
end
