//! Ранжирование волны: сетка × MERT с плавной деградацией.
//!
//! `score = mert_norm · (1 + λ·affinity)`. База — MERT (всё, что нашёл qdrant,
//! уже ранжируемо), сетка поднимает наверх. Не-граф треки оседают в хвост →
//! волна бесконечно и плавно «ухудшается»: сперва высокая близость сетки +
//! сильный MERT, потом ниже % сетки, в конце — чистый MERT без сетки.

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use super::cursor::WaveCursor;
use super::graph::Affinity;
use super::track_arm::TrackArmCandidate;

#[derive(Debug, Clone, Copy)]
pub struct TrackMeta {
    pub primary_artist: Option<Uuid>,
    /// Лежит ли трек на нашем S3. Не-`ok` не отдаём — иначе late-drop схлопывает
    /// выдачу (корень бага исчезающих карточек).
    pub storage_ok: bool,
}

#[derive(Debug, Clone)]
pub struct RankedTrack {
    pub sc_track_id: u64,
    pub score: f32,
    pub artist: Option<Uuid>,
}

#[allow(clippy::too_many_arguments)]
pub fn rank_and_pick(
    mert: &[TrackArmCandidate],
    affinity: &Affinity,
    disliked_artists: &HashSet<Uuid>,
    meta: &HashMap<u64, TrackMeta>,
    cursor: &WaveCursor,
    limit: usize,
    artist_cap: usize,
    lambda: f32,
) -> Vec<RankedTrack> {
    // min-max нормализация MERT z-score → [0,1], чтобы умножать на буст сетки.
    let (lo, hi) = mert
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), c| {
            (lo.min(c.score), hi.max(c.score))
        });
    let span = (hi - lo).max(1e-6);

    let mut scored: Vec<RankedTrack> = Vec::with_capacity(mert.len());
    for c in mert {
        let Some(m) = meta.get(&c.sc_track_id) else {
            continue; // не в каталоге
        };
        if !m.storage_ok {
            continue;
        }
        if let Some(a) = m.primary_artist {
            if disliked_artists.contains(&a) {
                continue; // диз-артист — режем даже в чистом MERT-хвосте
            }
        }
        let mert_norm = ((c.score - lo) / span).clamp(0.0, 1.0);
        let aff = m
            .primary_artist
            .and_then(|a| affinity.get(&a).copied())
            .unwrap_or(0.0);
        let score = mert_norm * (1.0 + lambda * aff);
        scored.push(RankedTrack {
            sc_track_id: c.sc_track_id,
            score,
            artist: m.primary_artist,
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
