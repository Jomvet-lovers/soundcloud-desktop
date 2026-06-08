//! Helpers поверх per-user state-mirror таблиц (`user_likes_tracks`,
//! `user_likes_playlists`, `user_followings`). Семантика wanted_state/progress
//! идентична между ними — выносим в один helper, чтобы каждый сервис не
//! копипастил UPSERT/SELECT/match-tree.

use sqlx::PgPool;

use crate::error::AppResult;

/// Метаданные конкретной mirror-таблицы. Имена статичные — это безопасно
/// подставляется через `format!`, никакой user-input в SQL не попадает.
#[derive(Debug, Clone, Copy)]
pub struct WantedMirror {
    pub table: &'static str,
    pub key_col: &'static str,
}

pub const LIKES_TRACKS: WantedMirror = WantedMirror {
    table: "user_likes_tracks",
    key_col: "sc_track_id",
};
pub const LIKES_PLAYLISTS: WantedMirror = WantedMirror {
    table: "user_likes_playlists",
    key_col: "playlist_urn",
};
pub const FOLLOWINGS: WantedMirror = WantedMirror {
    table: "user_followings",
    key_col: "target_user_urn",
};

/// Оптимистично выставить wanted_state=true.
/// - Нет строки → INSERT (wanted=true, progress=true).
/// - Была pending-unwant (wanted=false) → возвращаем к wanted без SC-вызова
///   (progress=false); inverse-dedup в sync_queue снимет парную мутацию.
/// - Уже wanted=true → no-op (синканный или ожидающий).
pub async fn set_wanted(pg: &PgPool, m: WantedMirror, user_id: &str, key: &str) -> AppResult<()> {
    // Канон ключа mirror — bare numeric (закрывает URN/bare split: /me/* раньше
    // писал URN, /users/{self}/* читал bare). Сессия НЕ канонизируется глобально,
    // чтобы не задеть history/dislikes/subscriptions — только эти таблицы.
    let user_id = crate::common::sc_ids::extract_sc_id(user_id);
    let sql = format!(
        "INSERT INTO {table} (user_id, {key_col}, wanted_state, progress) \
         VALUES ($1, $2, true, true) \
         ON CONFLICT (user_id, {key_col}) DO UPDATE SET \
             wanted_state = true, \
             progress = CASE WHEN {table}.wanted_state = false \
                             THEN false \
                             ELSE {table}.progress END",
        table = m.table,
        key_col = m.key_col,
    );
    sqlx::query(&sql)
        .bind(user_id)
        .bind(key)
        .execute(pg)
        .await?;
    Ok(())
}

/// Оптимистично снять wanted_state.
/// - Нет строки → no-op (нечего отменять).
/// - (progress=true, wanted=true) — pending wantted, ещё не отправлен SC: DELETE.
/// - (_, wanted=true) — синканный, ставим (wanted=false, progress=true);
///   Phase-3 refresh её не воскресит, в read-path она не попадает.
/// - wanted=false — уже pending unwant, no-op.
pub async fn clear_wanted(pg: &PgPool, m: WantedMirror, user_id: &str, key: &str) -> AppResult<()> {
    let user_id = crate::common::sc_ids::extract_sc_id(user_id);
    let select_sql = format!(
        "SELECT progress, wanted_state FROM {table} WHERE user_id = $1 AND {key_col} = $2",
        table = m.table,
        key_col = m.key_col,
    );
    let row: Option<(bool, bool)> = sqlx::query_as(&select_sql)
        .bind(user_id)
        .bind(key)
        .fetch_optional(pg)
        .await?;
    match row {
        None | Some((_, false)) => Ok(()),
        Some((true, true)) => {
            let sql = format!(
                "DELETE FROM {table} WHERE user_id = $1 AND {key_col} = $2",
                table = m.table,
                key_col = m.key_col,
            );
            sqlx::query(&sql)
                .bind(user_id)
                .bind(key)
                .execute(pg)
                .await?;
            Ok(())
        }
        Some((_, true)) => {
            let sql = format!(
                "UPDATE {table} SET wanted_state = false, progress = true \
                 WHERE user_id = $1 AND {key_col} = $2",
                table = m.table,
                key_col = m.key_col,
            );
            sqlx::query(&sql)
                .bind(user_id)
                .bind(key)
                .execute(pg)
                .await?;
            Ok(())
        }
    }
}
