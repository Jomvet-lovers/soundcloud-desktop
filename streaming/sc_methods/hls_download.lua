-- sc.hls_download — download + glue an hls track via the relay (mode B): fetch the
-- (already-signed CDN) m3u8, download every segment, concatenate, and return the
-- audio base64-encoded.
--
-- inputs:  { url = "<signed m3u8 cdn url>" }
-- output:  { ok = true, audio_b64 = "<base64 audio>", bytes = N, segments = N }
--          | { ok = false, reason = "no_segments" }
--
-- Segments in a SC m3u8 are pre-signed CDN URLs (no client_id needed). Failure
-- convention: error() -> the relay retries on the next client.

local m3u8_url = inputs.url
local resp = http({ url = m3u8_url, method = "GET" })
if resp.status ~= 200 then
  error("m3u8 status " .. tostring(resp.status))
end

-- Base for relative segment lines: the m3u8 url up to the last '/', query stripped.
local base = m3u8_url:gsub("%?.*$", ""):gsub("[^/]*$", "")

local function absolutize(u)
  if u:sub(1, 4) ~= "http" then return base .. u end
  return u
end

local parts = {}
local n = 0

-- fmp4 init segment (#EXT-X-MAP:URI="...") must be prepended, if present.
local init = resp.body:match('#EXT%-X%-MAP:URI="([^"]+)"')
if init ~= nil then
  local ir = http({ url = absolutize(init), method = "GET" })
  if ir.status ~= 200 then error("init status " .. tostring(ir.status)) end
  n = n + 1
  parts[n] = ir.body
end

for line in resp.body:gmatch("[^\r\n]+") do
  local seg = line:gsub("%s+$", "")
  if seg ~= "" and seg:sub(1, 1) ~= "#" then
    n = n + 1
    local r = http({ url = absolutize(seg), method = "GET" })
    if r.status ~= 200 then
      error("segment " .. n .. " status " .. tostring(r.status))
    end
    parts[n] = r.body
  end
end

if n == 0 then
  return { ok = false, reason = "no_segments" }
end

local audio = table.concat(parts)
return { ok = true, audio_b64 = b64encode(audio), bytes = #audio, segments = n }
