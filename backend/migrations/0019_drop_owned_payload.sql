-- Owned mirror больше не хранит raw SC payload — owned tracks/playlists
-- нормализуются в shared `tracks`/`playlists` (включая приватные), а mirror
-- держит только (user_id, key, sync_state). Read-path для owner идёт через
-- те же `project_many`/`project_to_sc_shape` что и public — owner просто
-- видит всё включая sharing='private', public callers фильтруют.

ALTER TABLE user_owned_playlists DROP COLUMN IF EXISTS payload;
ALTER TABLE user_owned_tracks    DROP COLUMN IF EXISTS payload;
