use sqlx::PgPool;

use crate::error::AppResult;

const POSITIVE_TYPES: &[&str] = &["like", "local_like", "playlist_add"];
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

#[derive(Debug, sqlx::FromRow)]
struct EventRow {
    sc_track_id: String,
    event_type: String,
    weight: f64,
    position_pct: Option<f32>,
    age_days: f32,
}

pub async fn load_user_signals(pg: &PgPool, sc_user_id: &str) -> AppResult<UserSignals> {
    let rows: Vec<EventRow> = sqlx::query_as(
        "SELECT sc_track_id, event_type, weight, position_pct,
                (EXTRACT(EPOCH FROM (NOW() - created_at)) / 86400.0)::real AS age_days
         FROM user_events
         WHERE sc_user_id = $1
           AND created_at > NOW() - INTERVAL '365 days'
         ORDER BY created_at DESC
         LIMIT $2",
    )
    .bind(sc_user_id)
    .bind(POSITIVE_LIMIT + NEGATIVE_LIMIT + PLAYED_LIMIT)
    .fetch_all(pg)
    .await?;

    let disliked_ids: Vec<String> = sqlx::query_scalar(
        "SELECT sc_track_id FROM disliked_tracks WHERE sc_user_id = $1 LIMIT 500",
    )
    .bind(sc_user_id)
    .fetch_all(pg)
    .await
    .unwrap_or_default();

    let disliked_set: std::collections::HashSet<String> = disliked_ids.iter().cloned().collect();

    let mut strong_positives: Vec<WeightedTrack> = Vec::new();
    let mut implicit_positives: Vec<WeightedTrack> = Vec::new();
    let mut played: Vec<String> = Vec::new();
    let mut negatives: Vec<WeightedTrack> = Vec::new();
    let mut seen_played = std::collections::HashSet::new();

    for r in rows {
        if disliked_set.contains(&r.sc_track_id) {
            continue;
        }
        let decay = decay_factor(r.age_days);
        let weighted_pos = WeightedTrack {
            sc_track_id: r.sc_track_id.clone(),
            weight: (r.weight.max(0.0) as f32) * decay,
        };
        let weighted_neg = WeightedTrack {
            sc_track_id: r.sc_track_id.clone(),
            weight: (r.weight.min(0.0).abs() as f32) * decay,
        };
        if POSITIVE_TYPES.contains(&r.event_type.as_str())
            && strong_positives.len() < POSITIVE_LIMIT as usize
        {
            strong_positives.push(weighted_pos);
        } else if r.event_type == IMPLICIT_POSITIVE
            && implicit_positives.len() < POSITIVE_LIMIT as usize
        {
            let multiplier = match r.position_pct {
                Some(p) if p >= 0.85 => 1.0,
                Some(p) if p >= 0.65 => 0.6,
                _ => 0.3,
            };
            implicit_positives.push(WeightedTrack {
                sc_track_id: r.sc_track_id.clone(),
                weight: weighted_pos.weight * multiplier,
            });
        }
        if NEGATIVE_TYPES.contains(&r.event_type.as_str())
            && negatives.len() < NEGATIVE_LIMIT as usize
        {
            negatives.push(weighted_neg);
        }
        if played.len() < PLAYED_LIMIT as usize && seen_played.insert(r.sc_track_id.clone()) {
            played.push(r.sc_track_id);
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

fn decay_factor(age_days: f32) -> f32 {
    if age_days.is_nan() || age_days < 0.0 {
        return 1.0;
    }
    (-age_days * std::f32::consts::LN_2 / DECAY_HALF_LIFE_DAYS).exp()
}
