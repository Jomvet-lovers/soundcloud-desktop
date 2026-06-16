-- sc.get_track — "relay, give me the track": in ONE call the relay does the whole
-- flow — track metadata -> pick transcoding -> resolve -> download (progressive /
-- glue hls / Widevine decrypt) -> return the audio. No per-step round-trips from the
-- service.
--
-- inputs:  { id = "<sc track id>", quality = "hq"|"sq" (opt, default sq),
--            wvd_url, wvd_token (opt — only for encrypted) }
-- output:  { ok = true, audio_b64, content_type, bytes, protocol }
--          | { ok = false, reason = "gone"|"no_media"|"no_transcoding"|"no_cdn_url"|... }
--
-- Failure convention: error() -> the relay retries on the next client.

local function get(url, hdrs)
  return http({ url = url, method = "GET", headers = hdrs })
end

local function absolutize(u, base)
  if u:sub(1, 4) ~= "http" then return base .. u end
  return u
end

-- progressive > hls > ctr-encrypted-hls; within a protocol, prefer the asked quality.
local function pick(transcodings, quality)
  for _, proto in ipairs({ "progressive", "hls", "ctr-encrypted-hls" }) do
    for _, want_q in ipairs({ quality, "hq", "sq" }) do
      for _, t in ipairs(transcodings) do
        if t.format ~= nil and t.format.protocol == proto and not t.snipped
            and t.quality == want_q then
          return t
        end
      end
    end
    for _, t in ipairs(transcodings) do
      if t.format ~= nil and t.format.protocol == proto and not t.snipped then
        return t
      end
    end
  end
  return nil
end

local function collect_hls(m3u8_url)
  local resp = get(m3u8_url)
  if resp.status ~= 200 then error("m3u8 status " .. tostring(resp.status)) end
  local base = m3u8_url:gsub("%?.*$", ""):gsub("[^/]*$", "")
  local parts, n = {}, 0
  local init = resp.body:match('#EXT%-X%-MAP:URI="([^"]+)"')
  if init ~= nil then
    local ir = get(absolutize(init, base))
    if ir.status ~= 200 then error("init status " .. tostring(ir.status)) end
    n = n + 1; parts[n] = ir.body
  end
  for line in resp.body:gmatch("[^\r\n]+") do
    local seg = line:gsub("%s+$", "")
    if seg ~= "" and seg:sub(1, 1) ~= "#" then
      n = n + 1
      local sr = get(absolutize(seg, base))
      if sr.status ~= 200 then error("segment " .. n .. " status " .. tostring(sr.status)) end
      parts[n] = sr.body
    end
  end
  if n == 0 then error("hls: no segments") end
  return table.concat(parts)
end

local cid = client_id()
if cid == nil or cid == "" then error("no client_id") end

-- 1. track metadata
local tr = get("https://api-v2.soundcloud.com/tracks/" .. urlencode(tostring(inputs.id))
  .. "?client_id=" .. urlencode(cid))
if tr.status == 404 then return { ok = false, reason = "gone" } end
if tr.status ~= 200 then error("track status " .. tostring(tr.status)) end
local track = json_decode(tr.body)
if type(track) ~= "table" or track.media == nil or track.media.transcodings == nil then
  return { ok = false, reason = "no_media" }
end

local chosen = pick(track.media.transcodings, inputs.quality or "sq")
if chosen == nil then return { ok = false, reason = "no_transcoding" } end
local proto = chosen.format.protocol
local content_type = chosen.format.mime_type or "audio/mpeg"

-- 2. resolve transcoding -> signed CDN url
local sep = "?"
if string.find(chosen.url, "?", 1, true) then sep = "&" end
local rt = chosen.url .. sep .. "client_id=" .. urlencode(cid)
if track.track_authorization ~= nil and track.track_authorization ~= "" then
  rt = rt .. "&track_authorization=" .. urlencode(track.track_authorization)
end
local rr = get(rt)
if rr.status ~= 200 then error("transcoding resolve status " .. tostring(rr.status)) end
local resolved = json_decode(rr.body)
if type(resolved) ~= "table" or resolved.url == nil then return { ok = false, reason = "no_cdn_url" } end

-- 3. download by protocol
if proto == "progressive" then
  local a = get(resolved.url)
  if a.status ~= 200 then error("progressive status " .. tostring(a.status)) end
  return { ok = true, audio_b64 = b64encode(a.body), content_type = content_type, bytes = #a.body, protocol = proto }
elseif proto == "hls" then
  local audio = collect_hls(resolved.url)
  return { ok = true, audio_b64 = b64encode(audio), content_type = content_type, bytes = #audio, protocol = proto }
elseif proto == "ctr-encrypted-hls" then
  if inputs.wvd_url == nil or inputs.wvd_url == "" then
    return { ok = false, reason = "encrypted_no_wvd" }
  end
  local wvd = get(inputs.wvd_url, { ["x-wvd-token"] = inputs.wvd_token })
  if wvd.status ~= 200 then error("wvd status " .. tostring(wvd.status)) end
  local manifest = get(resolved.url)
  if manifest.status ~= 200 then error("manifest status " .. tostring(manifest.status)) end
  local audio = widevine_decrypt(wvd.body, manifest.body, resolved.licenseAuthToken or "")
  return { ok = true, audio_b64 = b64encode(audio), content_type = "audio/mp4", bytes = #audio, protocol = proto }
else
  return { ok = false, reason = "unknown_protocol" }
end
