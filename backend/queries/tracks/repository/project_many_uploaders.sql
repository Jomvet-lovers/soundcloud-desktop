-- Uploader SC-shape JSON for project_many fan-in; one query instead of N JOINs.
SELECT sc_user_id,
       jsonb_build_object(
               'kind', 'user',
               'id', sc_user_id,
               'urn', urn,
               'username', username,
               'avatar_url', avatar_url,
               'permalink_url', permalink_url,
               'verified', verified,
               'country_code', country,
               'city', city,
               'description', description,
               'followers_count', followers_count,
               'followings_count', followings_count,
               'track_count', tracks_count
       ) AS "u!"
FROM users
WHERE sc_user_id = ANY ($1)
