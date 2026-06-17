-- sc.apiv2_get — generic apiv2 GET, returns the parsed JSON body.
--
-- The backend builds the full api-v2 URL (path + query); the client_id is appended here.
-- Used for paginated public lists (comments/reposters/related/followers/…) and cron
-- list-walks — the caller maps the body. SC's next_href omits client_id, so it is
-- (re)appended each page.
--
-- inputs:  { url = "https://api-v2.soundcloud.com/…" }
-- output:  { ok = true, data = <parsed json> } | { ok = false, reason = "gone" | "bad_body" }

local cid = client_id()
if cid == nil or cid == "" then
  error("no client_id")
end

if type(inputs.url) ~= "string" or inputs.url == "" then
  return { ok = false, reason = "bad_body" }
end

local url
if string.find(inputs.url, "?", 1, true) then
  url = inputs.url .. "&client_id=" .. urlencode(cid)
else
  url = inputs.url .. "?client_id=" .. urlencode(cid)
end

local resp = http({ url = url, method = "GET" })
if resp.status == 404 then
  return { ok = false, reason = "gone" }
elseif resp.status ~= 200 then
  error("apiv2_get status " .. tostring(resp.status))
end

local data = json_decode(resp.body)
if type(data) ~= "table" then
  return { ok = false, reason = "bad_body" }
end
return { ok = true, data = data }
