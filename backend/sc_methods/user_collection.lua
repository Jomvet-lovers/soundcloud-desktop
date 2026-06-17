-- sc.user_collection — one page of a public per-user apiv2 collection.
--
-- apiv2 like-feeds wrap each item as { created_at, kind, track|playlist }; we unwrap
-- to the bare entity so the backend maps it exactly like an apiv1 collection item.
-- The Rust side drives pagination by passing back `next_href` as `cursor` — SC's
-- next_href omits client_id, so it is (re)appended each page.
--
-- inputs:  { user_id = "183", kind = "track_likes"|"playlist_likes"|"playlists"
--                                     |"followings"|"tracks",
--            limit = 200, cursor = "<next_href>"? }
-- output:  { ok = true, collection = [ <bare apiv2 entity> ], next_href = "…"|nil }

local cid = client_id()
if cid == nil or cid == "" then
  error("no client_id")
end

local base = "https://api-v2.soundcloud.com"

-- apiv2 path + which field (if any) wraps the entity.
local SPEC = {
  track_likes    = { path = "/track_likes",    unwrap = "track" },
  playlist_likes = { path = "/playlist_likes", unwrap = "playlist" },
  playlists      = { path = "/playlists",      unwrap = nil },
  followings     = { path = "/followings",     unwrap = nil },
  tracks         = { path = "/tracks",         unwrap = nil },
}

local spec = SPEC[inputs.kind]
if spec == nil then
  return { ok = false, reason = "bad_kind" }
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
  local limit = tonumber(inputs.limit) or 200
  url = base .. "/users/" .. urlencode(tostring(inputs.user_id)) .. spec.path
    .. "?client_id=" .. urlencode(cid)
    .. "&limit=" .. tostring(limit)
    .. "&linked_partitioning=true"
end

local resp = http({ url = url, method = "GET" })
if resp.status == 404 then
  return { ok = false, reason = "gone" }
elseif resp.status ~= 200 then
  error("user_collection status " .. tostring(resp.status))
end

local data = json_decode(resp.body)
if type(data) ~= "table" then
  return { ok = false, reason = "bad_body" }
end

local out = {}
if type(data.collection) == "table" then
  for _, item in ipairs(data.collection) do
    local entity = item
    if spec.unwrap ~= nil and type(item) == "table" then
      entity = item[spec.unwrap]
    end
    if type(entity) == "table" and entity.id ~= nil then
      out[#out + 1] = entity
    end
  end
end

local next_href = data.next_href
if type(next_href) ~= "string" or next_href == "" then
  next_href = nil
end

return { ok = true, collection = out, next_href = next_href }
