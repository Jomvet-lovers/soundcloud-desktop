//! Ранжирование волны — КОНЪЮНКЦИЯ «И» по всем плоскостям, не «ИЛИ».
//!
//! Трек хорош, только если близок к вкусу ОДНОВРЕМЕННО по биту (MERT), вайбу
//! (CLAP), лирике (LYRICS) И по сетке (коллаб-граф). Контент-близость считается
//! как geomean этих плоскостей (`content`, см. mod.rs) — низкая близость по
//! ЛЮБОЙ оси топит трек. Сетка — множитель сверху (тоже «И»):
//!   `score = content · (graph_floor + (1-graph_floor)·affinity)`
//! `content < floor` → выкидываем (мисматч хотя бы по одной плоскости).

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
    /// Конъюнкция близости к вкусу по контент-плоскостям (geomean бит×вайб×лирика), [0..1].
    pub content: f32,
}

#[derive(Debug, Clone)]
pub struct RankedTrack {
    pub sc_track_id: u64,
    pub score: f32,
    pub artist: Option<Uuid>,
}

#[allow(clippy::too_many_arguments)]
pub fn rank_and_pick(
    cands: &[Candidate],
    affinity: &Affinity,
    disliked_artists: &HashSet<Uuid>,
    cursor: &WaveCursor,
    limit: usize,
    artist_cap: usize,
    graph_floor: f32,
    content_floor: f32,
) -> Vec<RankedTrack> {
    let mut scored: Vec<RankedTrack> = Vec::with_capacity(cands.len());
    for c in cands {
        if c.content < content_floor {
            continue; // мисматч хотя бы по одной плоскости
        }
        if let Some(a) = c.artist {
            if disliked_artists.contains(&a) {
                continue;
            }
        }
        let aff = c
            .artist
            .and_then(|a| affinity.get(&a).copied())
            .unwrap_or(0.0);
        // сетка тоже через «И» — множитель: non-graph → ×graph_floor.
        let graph_factor = graph_floor + (1.0 - graph_floor) * aff.clamp(0.0, 1.0);
        let score = c.content * graph_factor;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::recommendations::smart_wave::cursor::{SeedKind, WaveCursor};

    fn cursor() -> WaveCursor {
        WaveCursor::new(SeedKind::User, "test".into())
    }

    fn cand(id: u64, artist: u128, content: f32) -> Candidate {
        Candidate {
            sc_track_id: id,
            artist: Some(Uuid::from_u128(artist)),
            content,
        }
    }

    #[test]
    fn graph_track_outranks_alien_with_better_content() {
        // Сид-артист (aff 0.7, контент 0.7) против чужака вне сетки (контент 0.9).
        let mut aff: Affinity = HashMap::new();
        aff.insert(Uuid::from_u128(1), 0.7);
        let cands = vec![cand(10, 1, 0.7), cand(20, 2, 0.9)];
        let picked = rank_and_pick(&cands, &aff, &HashSet::new(), &cursor(), 2, 2, 0.12, 0.55);
        assert_eq!(picked[0].sc_track_id, 10);
        assert!(picked[0].score > picked[1].score * 3.0);
    }

    #[test]
    fn content_floor_drops_mismatch() {
        let aff: Affinity = HashMap::new();
        let cands = vec![cand(10, 1, 0.54), cand(20, 2, 0.56)];
        let picked = rank_and_pick(&cands, &aff, &HashSet::new(), &cursor(), 5, 2, 0.12, 0.55);
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].sc_track_id, 20);
    }

    #[test]
    fn disliked_artist_hard_dropped() {
        let mut aff: Affinity = HashMap::new();
        aff.insert(Uuid::from_u128(1), 1.0);
        let mut disliked = HashSet::new();
        disliked.insert(Uuid::from_u128(1));
        let cands = vec![cand(10, 1, 0.9)];
        let picked = rank_and_pick(&cands, &aff, &disliked, &cursor(), 5, 2, 0.12, 0.55);
        assert!(picked.is_empty());
    }

    #[test]
    fn artist_cap_in_window() {
        let mut aff: Affinity = HashMap::new();
        aff.insert(Uuid::from_u128(1), 1.0);
        let cands = vec![cand(10, 1, 0.9), cand(11, 1, 0.88), cand(12, 1, 0.86)];
        let picked = rank_and_pick(&cands, &aff, &HashSet::new(), &cursor(), 5, 2, 0.12, 0.55);
        assert_eq!(picked.len(), 2);
    }
}
