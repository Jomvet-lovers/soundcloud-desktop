-- Cold mirror of the SC /me profile, so /me/cold serves from DB instantly and
-- revalidates in the background. Keyed by SC user (one row per user, shared
-- across that user's sessions).
CREATE TABLE IF NOT EXISTS user_profiles
(
    soundcloud_user_id
    text
    PRIMARY
    KEY,
    profile_json
    jsonb
    NOT
    NULL,
    synced_at
    timestamptz
    NOT
    NULL
    DEFAULT
    now
(
)
    );
