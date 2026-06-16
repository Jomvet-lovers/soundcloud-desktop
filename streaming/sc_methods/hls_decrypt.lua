-- sc.hls_decrypt — decrypt a ctr-encrypted-hls (Widevine) track via the relay.
-- The relay fetches a .wvd device from this service, then runs the Widevine license
-- handshake + segment download + decrypt itself. Returns the clean fMP4 base64-encoded.
--
-- inputs:  { wvd_url = "...", wvd_token = "...", manifest = "<ctr m3u8 text>", token = "<licenseAuthToken>" }
-- output:  { ok = true, audio_b64 = "<base64 fMP4>", bytes = N }
--
-- Failure convention: error() -> the relay retries on the next client.

local wvd = http({
  url = inputs.wvd_url,
  method = "GET",
  headers = { ["x-wvd-token"] = inputs.wvd_token },
})
if wvd.status ~= 200 then
  error("wvd status " .. tostring(wvd.status))
end

-- widevine_decrypt does the license POST + segment GETs + decrypt, using this
-- client's HTTP. Returns the decrypted fMP4 bytes.
local audio = widevine_decrypt(wvd.body, inputs.manifest, inputs.token)

return { ok = true, audio_b64 = b64encode(audio), bytes = #audio }
