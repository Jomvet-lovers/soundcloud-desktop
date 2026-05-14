use std::collections::HashMap;

use serde_json::{Map, Value};
use sqlx::PgPool;

use crate::common::sc_ids::normalize_sc_track_id;
use crate::error::AppResult;

const STALE_SECS: i64 = 300;

#[derive(Debug, Clone, Copy, Default)]
struct Counters {
    play_count: Option<i64>,
    likes_count: Option<i64>,
    reposts_count: Option<i64>,
    comment_count: Option<i64>,
}

impl Counters {
    fn is_empty(&self) -> bool {
        self.play_count.is_none()
            && self.likes_count.is_none()
            && self.reposts_count.is_none()
            && self.comment_count.is_none()
    }
}

fn read_i64_from(obj: &Map<String, Value>, key: &str) -> Option<i64> {
    let n = obj.get(key)?;
    if n.is_null() {
        return None;
    }
    n.as_i64().or_else(|| n.as_u64().map(|u| u as i64))
}

fn read_counters_obj(obj: &Map<String, Value>) -> Counters {
    Counters {
        play_count: read_i64_from(obj, "playback_count")
            .or_else(|| read_i64_from(obj, "play_count")),
        likes_count: read_i64_from(obj, "likes_count")
            .or_else(|| read_i64_from(obj, "favoritings_count")),
        reposts_count: read_i64_from(obj, "reposts_count"),
        comment_count: read_i64_from(obj, "comment_count"),
    }
}

fn read_counters(v: &Value) -> Counters {
    v.as_object().map(read_counters_obj).unwrap_or_default()
}

pub async fn sync(pg: &PgPool, tracks: &mut [Value]) -> AppResult<()> {
    if tracks.is_empty() {
        return Ok(());
    }

    let mut fresh: HashMap<String, Counters> = HashMap::new();
    let mut sc_ids: Vec<String> = Vec::with_capacity(tracks.len());
    for t in tracks.iter() {
        let Some(urn) = t.get("urn").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(sc_id) = normalize_sc_track_id(urn) else {
            continue;
        };
        let c = read_counters(t);
        if !c.is_empty() {
            fresh.insert(sc_id.clone(), c);
        }
        sc_ids.push(sc_id);
    }
    if sc_ids.is_empty() {
        return Ok(());
    }

    if !fresh.is_empty() {
        let mut entries: Vec<(String, Counters)> =
            fresh.iter().map(|(id, c)| (id.clone(), *c)).collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        let mut ids: Vec<String> = Vec::with_capacity(entries.len());
        let mut play: Vec<Option<i64>> = Vec::with_capacity(entries.len());
        let mut likes: Vec<Option<i64>> = Vec::with_capacity(entries.len());
        let mut reposts: Vec<Option<i64>> = Vec::with_capacity(entries.len());
        let mut comments: Vec<Option<i64>> = Vec::with_capacity(entries.len());
        for (id, c) in entries {
            ids.push(id);
            play.push(c.play_count);
            likes.push(c.likes_count);
            reposts.push(c.reposts_count);
            comments.push(c.comment_count);
        }
        // ORDER BY u.id keeps row-level locks in the same order across
        // concurrent transactions inserting overlapping key sets — without it
        // UNNEST'd batches deadlock under load.
        sqlx::query(
            "INSERT INTO sc_track_counters (sc_track_id, play_count, likes_count, reposts_count, comment_count, fetched_at)
             SELECT u.id, u.p, u.l, u.r, u.c, now()
             FROM UNNEST($1::text[], $2::bigint[], $3::bigint[], $4::bigint[], $5::bigint[]) AS u(id, p, l, r, c)
             ORDER BY u.id
             ON CONFLICT (sc_track_id) DO UPDATE SET
                play_count    = COALESCE(EXCLUDED.play_count, sc_track_counters.play_count),
                likes_count   = COALESCE(EXCLUDED.likes_count, sc_track_counters.likes_count),
                reposts_count = COALESCE(EXCLUDED.reposts_count, sc_track_counters.reposts_count),
                comment_count = COALESCE(EXCLUDED.comment_count, sc_track_counters.comment_count),
                fetched_at    = now()",
        )
        .bind(&ids)
        .bind(&play)
        .bind(&likes)
        .bind(&reposts)
        .bind(&comments)
        .execute(pg)
        .await?;
    }

    let rows: Vec<(
        String,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        chrono::DateTime<chrono::Utc>,
    )> = sqlx::query_as(
        "SELECT sc_track_id, play_count, likes_count, reposts_count, comment_count, fetched_at
             FROM sc_track_counters WHERE sc_track_id = ANY($1)",
    )
    .bind(&sc_ids)
    .fetch_all(pg)
    .await?;
    if rows.is_empty() {
        return Ok(());
    }
    let now = chrono::Utc::now();
    let stored: HashMap<String, (Counters, i64)> = rows
        .into_iter()
        .map(|(id, p, l, r, c, fetched)| {
            let age = (now - fetched).num_seconds();
            (
                id,
                (
                    Counters {
                        play_count: p,
                        likes_count: l,
                        reposts_count: r,
                        comment_count: c,
                    },
                    age,
                ),
            )
        })
        .collect();

    for t in tracks.iter_mut() {
        let Some(urn) = t.get("urn").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(sc_id) = normalize_sc_track_id(urn) else {
            continue;
        };
        let Some((c, age)) = stored.get(&sc_id) else {
            continue;
        };
        let Some(obj) = t.as_object_mut() else {
            continue;
        };
        let stale = *age > STALE_SECS;
        let already_present = read_counters_obj(obj);
        if let Some(v) = c.play_count {
            if already_present.play_count.is_none()
                || (!stale && already_present.play_count != Some(v))
            {
                obj.insert("playback_count".into(), Value::from(v));
            }
        }
        if let Some(v) = c.likes_count {
            if already_present.likes_count.is_none()
                || (!stale && already_present.likes_count != Some(v))
            {
                obj.insert("likes_count".into(), Value::from(v));
                obj.insert("favoritings_count".into(), Value::from(v));
            }
        }
        if let Some(v) = c.reposts_count {
            if already_present.reposts_count.is_none()
                || (!stale && already_present.reposts_count != Some(v))
            {
                obj.insert("reposts_count".into(), Value::from(v));
            }
        }
        if let Some(v) = c.comment_count {
            if already_present.comment_count.is_none()
                || (!stale && already_present.comment_count != Some(v))
            {
                obj.insert("comment_count".into(), Value::from(v));
            }
        }
    }
    Ok(())
}
