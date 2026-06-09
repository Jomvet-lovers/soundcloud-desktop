SELECT it.sc_track_id       AS "sc_track_id!",
       it.id                AS "track_id!",
       it.enrich_state      AS "enrich_state!",
       it.enrich_source     AS "enrich_source",
       it.enrich_confidence AS "enrich_confidence",
       it.upload_kind       AS "upload_kind!",
       it.release_year      AS "it_release_year",
       it.release_date      AS "it_release_date",
       a.id                 AS "pa_id?",
       a.name               AS "pa_name?",
       a.avatar_url         AS "pa_avatar_url?",
       a.sc_user_id         AS "pa_sc_user_id?",
       a.source             AS "pa_source?",
       a.confidence         AS "pa_confidence?",
       al.id                AS "al_id?",
       al.title             AS "al_title?",
       al.release_year      AS "al_release_year?",
       al.cover_url         AS "al_cover_url?",
       al.type              AS "al_kind?",
       aa.id                AS "aa_id?",
       aa.name              AS "aa_name?",
       aa.avatar_url        AS "aa_avatar_url?",
       aa.sc_user_id        AS "aa_sc_user_id?",
       aa.source            AS "aa_source?",
       aa.confidence        AS "aa_confidence?"
FROM tracks it
         LEFT JOIN artists a ON a.id = it.primary_artist_id
         LEFT JOIN albums al ON al.id = it.album_id
         LEFT JOIN artists aa ON aa.id = al.primary_artist_id
WHERE it.sc_track_id = ANY ($1)
