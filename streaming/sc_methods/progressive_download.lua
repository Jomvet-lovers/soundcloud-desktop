-- sc.progressive_download — download a progressive (single-file) track via the relay,
-- returning the audio base64-encoded.
--
-- inputs:  { url = "<signed progressive cdn url>" }
-- output:  { ok = true, audio_b64 = "<base64 audio>", bytes = N } | { ok = false, reason = "gone" }
--
-- Failure convention: error() -> the relay retries on the next client.

local resp = http({ url = inputs.url, method = "GET" })
local s = resp.status

if s == 200 then
  return { ok = true, audio_b64 = b64encode(resp.body), bytes = #resp.body }
elseif s == 404 then
  return { ok = false, reason = "gone" }
else
  error("progressive status " .. tostring(s))
end
