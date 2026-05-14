use std::collections::HashSet;

use serde_json::Value;
use sqlx::PgPool;

use crate::common::sc_ids::extract_sc_id;
use crate::error::AppResult;

/// Подмножество sc_track_id из переданного списка urns, которые юзер залайкал
/// (wanted_state=true — pending unlike исключены).
async fn fetch_liked_ids(
    pg: &PgPool,
    sc_user_id: &str,
    sc_track_ids: &[String],
) -> AppResult<HashSet<String>> {
    if sc_track_ids.is_empty() {
        return Ok(HashSet::new());
    }
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT sc_track_id FROM user_likes_tracks \
         WHERE user_id = $1 AND wanted_state = true AND sc_track_id = ANY($2)",
    )
    .bind(sc_user_id)
    .bind(sc_track_ids)
    .fetch_all(pg)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Подмешать `user_favorite=true` к трекам, которые есть в user_likes_tracks.
pub async fn apply_user_favorite_flag(
    pg: &PgPool,
    sc_user_id: &str,
    tracks: &mut [Value],
) -> AppResult<()> {
    let ids: Vec<String> = tracks
        .iter()
        .filter_map(|t| t.get("urn").and_then(|v| v.as_str()).map(extract_sc_id))
        .map(String::from)
        .collect();
    if ids.is_empty() {
        return Ok(());
    }
    let liked_ids = fetch_liked_ids(pg, sc_user_id, &ids).await?;
    if liked_ids.is_empty() {
        return Ok(());
    }
    for t in tracks.iter_mut() {
        let liked = t
            .get("urn")
            .and_then(|v| v.as_str())
            .is_some_and(|u| liked_ids.contains(extract_sc_id(u)));
        if liked {
            if let Some(obj) = t.as_object_mut() {
                obj.insert("user_favorite".into(), Value::Bool(true));
            }
        }
    }
    Ok(())
}

/// То же что `apply_user_favorite_flag`, но для activity-items с track-origin
/// (используется в /me/feed*). Мутирует `origin` каждой activity inline,
/// без клонирования track-объекта.
pub async fn apply_user_favorite_flag_to_activities(
    pg: &PgPool,
    sc_user_id: &str,
    activities: &mut [Value],
) -> AppResult<()> {
    let ids: Vec<String> = activities
        .iter()
        .filter_map(|a| a.get("origin"))
        .filter(|o| o.get("kind").and_then(|k| k.as_str()) == Some("track"))
        .filter_map(|o| o.get("urn").and_then(|v| v.as_str()).map(extract_sc_id))
        .map(String::from)
        .collect();
    if ids.is_empty() {
        return Ok(());
    }
    let liked_ids = fetch_liked_ids(pg, sc_user_id, &ids).await?;
    if liked_ids.is_empty() {
        return Ok(());
    }
    for a in activities.iter_mut() {
        let Some(origin) = a.get_mut("origin") else {
            continue;
        };
        if origin.get("kind").and_then(|k| k.as_str()) != Some("track") {
            continue;
        }
        let liked = origin
            .get("urn")
            .and_then(|v| v.as_str())
            .is_some_and(|u| liked_ids.contains(extract_sc_id(u)));
        if liked {
            if let Some(obj) = origin.as_object_mut() {
                obj.insert("user_favorite".into(), Value::Bool(true));
            }
        }
    }
    Ok(())
}
