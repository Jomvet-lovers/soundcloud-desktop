-- tx-level advisory lock keyed by "playlist_tracks:{urn}"
SELECT pg_advisory_xact_lock(hashtextextended($1, 0))
