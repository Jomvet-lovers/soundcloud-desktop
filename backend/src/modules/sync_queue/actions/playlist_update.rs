//! Legacy action-type. Новые правки идут через `playlist_sync` (local-first,
//! невырушающий). KIND оставлен, чтобы in-flight прод-строки `playlist_update`
//! дренировались алиасом на `playlist_sync::execute` (см. dispatch).

pub const KIND: &str = "playlist_update";
