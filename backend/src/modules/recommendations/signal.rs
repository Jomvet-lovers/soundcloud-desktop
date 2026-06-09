use std::collections::HashSet;

use sqlx::PgPool;

use crate::error::AppResult;

const IMPLICIT_POSITIVE: &str = "full_play";
const NEGATIVE_TYPES: &[&str] = &["dislike", "skip"];

const DECAY_HALF_LIFE_DAYS: f32 = 90.0;
const POSITIVE_LIMIT: i64 = 80;
const NEGATIVE_LIMIT: i64 = 200;
const PLAYED_LIMIT: i64 = 300;
const STRONG_POSITIVE_MIN: usize = 8;
const IMPLICIT_POSITIVE_MIN: usize = 12;
const PLAYED_FALLBACK_MIN: usize = 20;

#[derive(Debug, Clone)]
pub struct WeightedTrack {
    pub sc_track_id: String,
    pub weight: f32,
}

#[derive(Debug, Default)]
pub struct UserSignals {
    pub strong_positives: Vec<WeightedTrack>,
    pub implicit_positives: Vec<WeightedTrack>,
    pub played: Vec<String>,
    pub negatives: Vec<WeightedTrack>,
    pub disliked_ids: Vec<String>,
}

impl UserSignals {
    pub fn best_seed_kind(&self) -> SeedKind {
        if self.strong_positives.len() >= STRONG_POSITIVE_MIN {
            SeedKind::Strong
        } else if self.implicit_positives.len() >= IMPLICIT_POSITIVE_MIN {
            SeedKind::Implicit
        } else if self.played.len() >= PLAYED_FALLBACK_MIN {
            SeedKind::Played
        } else {
            SeedKind::ColdStart
        }
    }

    pub fn positive_seed(&self) -> Vec<WeightedTrack> {
        match self.best_seed_kind() {
            SeedKind::Strong => self.strong_positives.clone(),
            SeedKind::Implicit => {
                let mut out = self.strong_positives.clone();
                out.extend(self.implicit_positives.iter().cloned());
                out
            }
            SeedKind::Played => self
                .played
                .iter()
                .map(|id| WeightedTrack {
                    sc_track_id: id.clone(),
                    weight: 0.1,
                })
                .collect(),
            SeedKind::ColdStart => Vec::new(),
        }
    }

    pub fn has_any_signal(&self) -> bool {
        !self.strong_positives.is_empty()
            || !self.implicit_positives.is_empty()
            || !self.played.is_empty()
            || !self.negatives.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedKind {
    Strong,
    Implicit,
    Played,
    ColdStart,
}

pub async fn load_user_signals(pg: &PgPool, sc_user_id: &str) -> AppResult<UserSignals> {
    let variants = crate::common::sc_ids::user_id_variants(sc_user_id);
    let disliked_ids: Vec<String> =
        sqlx::query_file_scalar!("queries/recommendations/signal/disliked_ids.sql", &variants)
            .fetch_all(pg)
            .await
            .unwrap_or_default();
    let disliked_set: HashSet<String> = disliked_ids.iter().cloned().collect();

    let strong_positives = load_strong_positives(pg, sc_user_id, &disliked_set).await;

    let event_filter: &[&str] = &[IMPLICIT_POSITIVE, "skip", "dislike"];
    let event_rows = sqlx::query_file!(
        "queries/recommendations/signal/event_rows.sql",
        &variants,
        event_filter as &[&str],
        NEGATIVE_LIMIT + PLAYED_LIMIT
    )
    .fetch_all(pg)
    .await
    .unwrap_or_default();

    let mut implicit_positives: Vec<WeightedTrack> = Vec::new();
    let mut played: Vec<String> = Vec::new();
    let mut negatives: Vec<WeightedTrack> = Vec::new();
    let mut seen_played: HashSet<String> = strong_positives
        .iter()
        .map(|w| w.sc_track_id.clone())
        .collect();

    for r in event_rows {
        if disliked_set.contains(&r.sc_track_id) {
            continue;
        }
        let decay = decay_factor(r.age_days);
        if r.event_type == IMPLICIT_POSITIVE && implicit_positives.len() < POSITIVE_LIMIT as usize {
            let multiplier = match r.position_pct {
                Some(p) if p >= 0.85 => 1.0,
                Some(p) if p >= 0.65 => 0.6,
                _ => 0.3,
            };
            implicit_positives.push(WeightedTrack {
                sc_track_id: r.sc_track_id.clone(),
                weight: (r.weight.max(0.0) as f32) * decay * multiplier,
            });
        }
        if NEGATIVE_TYPES.contains(&r.event_type.as_str())
            && negatives.len() < NEGATIVE_LIMIT as usize
        {
            negatives.push(WeightedTrack {
                sc_track_id: r.sc_track_id.clone(),
                weight: (r.weight.min(0.0).abs() as f32) * decay,
            });
        }
        if played.len() < PLAYED_LIMIT as usize && seen_played.insert(r.sc_track_id.clone()) {
            played.push(r.sc_track_id);
        }
    }

    for w in &strong_positives {
        if played.len() < PLAYED_LIMIT as usize && seen_played.insert(w.sc_track_id.clone()) {
            played.push(w.sc_track_id.clone());
        }
    }

    for id in &disliked_ids {
        if negatives.iter().all(|n| &n.sc_track_id != id) {
            negatives.push(WeightedTrack {
                sc_track_id: id.clone(),
                weight: 1.0,
            });
        }
    }

    Ok(UserSignals {
        strong_positives,
        implicit_positives,
        played,
        negatives,
        disliked_ids,
    })
}

/// Лайки приоритетно тянем из `user_events` — это реальные click-actions с
/// весом. Если их меньше порога (например, свежий юзер, у которого только
/// синканулось зеркало `/me/likes/tracks`) — добираем недостающее из
/// `user_likes_tracks` тем же порядком, что отдаёт зеркало:
/// `ORDER BY created_at DESC, ctid DESC` (ctid резолвит ties в батче refresh'а).
async fn load_strong_positives(
    pg: &PgPool,
    sc_user_id: &str,
    disliked: &HashSet<String>,
) -> Vec<WeightedTrack> {
    let mut out: Vec<WeightedTrack> = Vec::with_capacity(POSITIVE_LIMIT as usize);
    let mut seen: HashSet<String> = HashSet::new();
    let variants = crate::common::sc_ids::user_id_variants(sc_user_id);

    let event_likes = sqlx::query_file!(
        "queries/recommendations/signal/event_likes.sql",
        &variants,
        POSITIVE_LIMIT
    )
    .fetch_all(pg)
    .await
    .unwrap_or_default();

    for r in event_likes {
        if disliked.contains(&r.sc_track_id) || !seen.insert(r.sc_track_id.clone()) {
            continue;
        }
        let decay = decay_factor(r.age_days);
        out.push(WeightedTrack {
            sc_track_id: r.sc_track_id,
            weight: (r.weight.max(0.0) as f32) * decay,
        });
        if out.len() >= POSITIVE_LIMIT as usize {
            return out;
        }
    }

    if out.len() >= STRONG_POSITIVE_MIN {
        return out;
    }

    // Fallback: зеркало `/me/likes/tracks`. Сортируем как зеркало
    // (ORDER BY created_at DESC, ctid DESC) — свежий лайк приоритетный.
    let need_more = (POSITIVE_LIMIT as usize).saturating_sub(out.len());
    let mirror_likes = sqlx::query_file!(
        "queries/recommendations/signal/mirror_likes.sql",
        &variants,
        need_more as i64
    )
    .fetch_all(pg)
    .await
    .unwrap_or_default();

    for r in mirror_likes {
        if disliked.contains(&r.sc_track_id) || !seen.insert(r.sc_track_id.clone()) {
            continue;
        }
        out.push(WeightedTrack {
            sc_track_id: r.sc_track_id,
            weight: decay_factor(r.age_days),
        });
        if out.len() >= POSITIVE_LIMIT as usize {
            break;
        }
    }

    out
}

fn decay_factor(age_days: f32) -> f32 {
    if age_days.is_nan() || age_days < 0.0 {
        return 1.0;
    }
    (-age_days * std::f32::consts::LN_2 / DECAY_HALF_LIFE_DAYS).exp()
}
