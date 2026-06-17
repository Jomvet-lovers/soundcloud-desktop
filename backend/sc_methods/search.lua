-- sc.search — one page of an apiv2 search of a given type.
--
-- apiv2 returns full, bare objects in `collection` (+ total_results, next_href), so no
-- unwrap is needed. The Rust side drives pagination via `next_href` (client_id omitted
-- by SC, so it is (re)appended each page).
--
-- inputs:  { type = "tracks"|"users"|"playlists_without_albums"|"albums",
--            q = "query", limit = 20, cursor = "<next_href>"? }
-- output:  { ok = true, collection = [ <apiv2 entity> ], next_href = "…"|nil,
--            total_results = N|nil }

local cid = client_id()
if cid == nil or cid == "" then
  error("no client_id")
end

local base = "https://api-v2.soundcloud.com"

local ALLOWED = {
  tracks = true,
  users = true,
  playlists_without_albums = true,
  albums = true,
}
if ALLOWED[inputs.type] ~= true then
  return { ok = false, reason = "bad_type" }
end

local function with_client_id(u)
  if string.find(u, "?", 1, true) then
    return u .. "&client_id=" .. urlencode(cid)
  end
  return u .. "?client_id=" .. urlencode(cid)
end

local url
if type(inputs.cursor) == "string" and inputs.cursor ~= "" then
  url = with_client_id(inputs.cursor)
else
  local limit = tonumber(inputs.limit) or 20
  url = base .. "/search/" .. inputs.type
    .. "?client_id=" .. urlencode(cid)
    .. "&q=" .. urlencode(tostring(inputs.q or ""))
    .. "&limit=" .. tostring(limit)
    .. "&linked_partitioning=true"
end

local resp = http({ url = url, method = "GET" })
if resp.status ~= 200 then
  error("search status " .. tostring(resp.status))
end

local data = json_decode(resp.body)
if type(data) ~= "table" then
  return { ok = false, reason = "bad_body" }
end

local out = {}
if type(data.collection) == "table" then
  for _, item in ipairs(data.collection) do
    if type(item) == "table" and item.id ~= nil then
      out[#out + 1] = item
    end
  end
end

local next_href = data.next_href
if type(next_href) ~= "string" or next_href == "" then
  next_href = nil
end

return { ok = true, collection = out, next_href = next_href, total_results = data.total_results }
