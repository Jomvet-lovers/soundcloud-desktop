-- sc.playlist_full — apiv2 playlist + its full, ordered track list in one call.
--
-- apiv2 embeds only ~5 full tracks in a playlist; the rest are {id, kind} stubs.
-- The edge client fetches the playlist, collects track ids in order, batch-hydrates
-- the missing ones via /tracks?ids=… (≤50/req, order mapped by id), and returns the
-- playlist with `tracks` replaced by the full ordered list.
--
-- inputs:  { id = "123456", hydrate = true }   (bare playlist id; hydrate defaults true,
--                                                false = meta only, skip /tracks batch)
-- output:  { ok = true, playlist = <apiv2 playlist, tracks hydrated> }
--          | { ok = false, reason = "no_playlist" | "gone" }

local cid = client_id()
if cid == nil or cid == "" then
  error("no client_id")
end

local base = "https://api-v2.soundcloud.com"

local function get_playlist(id)
  local url = base .. "/playlists/" .. urlencode(tostring(id)) .. "?client_id=" .. urlencode(cid)
  local resp = http({ url = url, method = "GET" })
  if resp.status == 200 then
    return json_decode(resp.body), nil
  elseif resp.status == 404 then
    return nil, "gone"
  else
    error("playlist status " .. tostring(resp.status))
  end
end

-- Hydrate a list of ids into a map id->full track. Missing/deleted ids are simply
-- absent from the map (SC drops them from the response).
local function hydrate(ids)
  local by_id = {}
  local i = 1
  local n = #ids
  while i <= n do
    local last = math.min(i + 49, n)
    local parts = {}
    for k = i, last do
      parts[#parts + 1] = tostring(ids[k])
    end
    local url = base .. "/tracks?ids=" .. table.concat(parts, ",") .. "&client_id=" .. urlencode(cid)
    local resp = http({ url = url, method = "GET" })
    if resp.status ~= 200 then
      error("hydrate status " .. tostring(resp.status))
    end
    local arr = json_decode(resp.body)
    if type(arr) == "table" then
      for _, t in ipairs(arr) do
        if type(t) == "table" and t.id ~= nil then
          by_id[tostring(t.id)] = t
        end
      end
    end
    i = last + 1
  end
  return by_id
end

local pl, reason = get_playlist(inputs.id)
if pl == nil then
  return { ok = false, reason = reason or "no_playlist" }
end
if type(pl) ~= "table" or pl.id == nil then
  return { ok = false, reason = "no_playlist" }
end

if inputs.hydrate == false then
  return { ok = true, playlist = pl }
end

-- Collect ids in playlist order; keep any embedded full objects (those with a title).
local ids = {}
local have = {}
if type(pl.tracks) == "table" then
  for _, t in ipairs(pl.tracks) do
    if type(t) == "table" and t.id ~= nil then
      local key = tostring(t.id)
      ids[#ids + 1] = key
      if t.title ~= nil then
        have[key] = t
      end
    end
  end
end

local missing = {}
for _, key in ipairs(ids) do
  if have[key] == nil then
    missing[#missing + 1] = key
  end
end
local fetched = hydrate(missing)

local ordered = {}
for _, key in ipairs(ids) do
  local full = have[key] or fetched[key]
  if full ~= nil then
    ordered[#ordered + 1] = full
  end
end
pl.tracks = ordered

return { ok = true, playlist = pl }
