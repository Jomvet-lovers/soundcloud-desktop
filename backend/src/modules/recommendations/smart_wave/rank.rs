//! Ранжирование волны: сетка + MERT с плавной деградацией.
//!
//! Кандидаты идут из ДВУХ источников: сетка (треки близких артистов) и MERT
//! (qdrant-похожие на лайки). Скор аддитивный:
//!   `score = aff·W_graph + mert_norm·W_mert + aff·mert_norm·W_syn`
//! - в топе: высокая близость сетки И сильный MERT (синергия);
//! - ниже: меньше % сетки;
//! - в хвосте: без сетки (чистый MERT, `aff=0`).
//! Так волна и бесконечна, и плавно «ухудшается».

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use super::cursor::WaveCursor;
use super::graph::Affinity;

#[derive(Debug, Clone, Copy)]
pub struct TrackMeta {
    pub primary_artist: Option<Uuid>,
    /// Лежит ли трек на нашем S3 — не-`ok` не отдаём (иначе late-drop схлопывает выдачу).
    pub storage_ok: bool,
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub sc_track_id: u64,
    pub artist: Option<Uuid>,
    /// raw z-score из qdrant; `None` = трек пришёл только из сетки.
    pub mert: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct RankedTrack {
    pub sc_track_id: u64,
    pub score: f32,
    pub artist: Option<Uuid>,
}

#[derive(Debug, Clone, Copy)]
pub struct RankWeights {
    pub graph: f32,
    pub mert: f32,
    pub synergy: f32,
}

pub fn rank_and_pick(
    cands: &[Candidate],
    affinity: &Affinity,
    disliked_artists: &HashSet<Uuid>,
    cursor: &WaveCursor,
    limit: usize,
    artist_cap: usize,
    w: RankWeights,
) -> Vec<RankedTrack> {
    // min-max нормализация MERT z-score → [0,1] по тем кандидатам, у кого он есть.
    let (lo, hi) = cands
        .iter()
        .filter_map(|c| c.mert)
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), m| {
            (lo.min(m), hi.max(m))
        });
    let span = (hi - lo).max(1e-6);

    let mut scored: Vec<RankedTrack> = Vec::with_capacity(cands.len());
    for c in cands {
        if let Some(a) = c.artist {
            if disliked_artists.contains(&a) {
                continue; // диз-артист — режем даже в чистом MERT
            }
        }
        let aff = c
            .artist
            .and_then(|a| affinity.get(&a).copied())
            .unwrap_or(0.0);
        let mert_norm = c.mert.map(|m| ((m - lo) / span).clamp(0.0, 1.0));
        // трек без сетки И без MERT — мусор.
        if aff <= 0.0 && mert_norm.is_none() {
            continue;
        }
        let mn = mert_norm.unwrap_or(0.0);
        let score = aff * w.graph + mn * w.mert + aff * mn * w.synergy;
        scored.push(RankedTrack {
            sc_track_id: c.sc_track_id,
            score,
            artist: c.artist,
        });
    }
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // дедуп по курсору + artist-cap в скользящем окне (анти-моно).
    let mut out: Vec<RankedTrack> = Vec::with_capacity(limit);
    let mut local_artist: HashMap<Uuid, usize> = HashMap::new();
    let mut seen: HashSet<u64> = HashSet::new();
    for r in scored {
        if out.len() >= limit {
            break;
        }
        if cursor.contains(r.sc_track_id) || !seen.insert(r.sc_track_id) {
            continue;
        }
        if let Some(a) = r.artist {
            let used =
                cursor.artist_count_in_window(a) + local_artist.get(&a).copied().unwrap_or(0);
            if used >= artist_cap {
                continue;
            }
            *local_artist.entry(a).or_insert(0) += 1;
        }
        out.push(r);
    }
    out
}
