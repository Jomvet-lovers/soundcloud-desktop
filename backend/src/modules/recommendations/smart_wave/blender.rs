//! Blender: смешивает track-arm, artist-arm и collab-arm в бесконечную волну
//! с адаптацией к фидбеку юзера.
//!
//! Веса начинают как 0.55/0.35/0.10. При негативной реакции (neg_rate > 0.3
//! в окне 20 треков) перевешиваем в сторону artist-arm — это и есть твоё
//! "если много дизов на ии-реки, то на близость артистов".

use std::collections::{HashMap, HashSet};

use crate::modules::recommendations::service::RecommendationsService;

use super::artist_arm::ArtistArmCandidate;
use super::cursor::WaveCursor;
use super::track_arm::TrackArmCandidate;

#[derive(Debug, Clone, Copy)]
pub struct BlendWeights {
    pub track: f32,
    pub artist: f32,
    pub collab: f32,
}

impl BlendWeights {
    pub fn default_user() -> Self {
        Self {
            track: 0.55,
            artist: 0.35,
            collab: 0.10,
        }
    }

    pub fn for_track_seed() -> Self {
        // Когда волна стартует "от трека" — track-arm доминирует, потому что
        // якорь и есть seed-трек, artist-graph здесь играет роль расширения.
        Self {
            track: 0.65,
            artist: 0.25,
            collab: 0.10,
        }
    }

    pub fn for_artist_seed() -> Self {
        // "От артиста" — наоборот, artist-graph определяет вкус сетки.
        Self {
            track: 0.35,
            artist: 0.55,
            collab: 0.10,
        }
    }

    pub fn adapt_to_negative(self, neg_rate: f32) -> Self {
        if neg_rate <= 0.3 {
            return self;
        }
        // Сдвигаем 0.15 трек-арма в сторону artist + collab.
        let shift = ((neg_rate - 0.3) * 0.5).clamp(0.0, 0.20);
        let track = (self.track - shift).clamp(0.20, 1.0);
        let artist = (self.artist + shift * 0.7).clamp(0.0, 0.7);
        let collab = (self.collab + shift * 0.3).clamp(0.0, 0.5);
        let total = track + artist + collab;
        Self {
            track: track / total,
            artist: artist / total,
            collab: collab / total,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BlendedCandidate {
    pub sc_track_id: u64,
    pub score: f32,
}

/// Финальный микс: нормализуем каждый arm в [0,1] по max и сливаем
/// взвешенно. Дубликаты (один и тот же трек предложили разные arms) суммируем
/// — это естественный буст.
pub fn blend(
    track_arm: &[TrackArmCandidate],
    artist_arm: &[ArtistArmCandidate],
    collab_arm: &[(u64, f32)],
    weights: BlendWeights,
) -> Vec<BlendedCandidate> {
    let track_max = track_arm.iter().map(|c| c.score).fold(0f32, f32::max).max(1e-6);
    let artist_max = artist_arm.iter().map(|c| c.score).fold(0f32, f32::max).max(1e-6);
    let collab_max = collab_arm.iter().map(|(_, s)| *s).fold(0f32, f32::max).max(1e-6);

    let mut by_id: HashMap<u64, f32> = HashMap::new();
    for c in track_arm {
        let s = (c.score / track_max) * weights.track;
        *by_id.entry(c.sc_track_id).or_insert(0.0) += s;
    }
    for c in artist_arm {
        let s = (c.score / artist_max) * weights.artist;
        *by_id.entry(c.sc_track_id).or_insert(0.0) += s;
    }
    for (id, score) in collab_arm {
        let s = (score / collab_max) * weights.collab;
        *by_id.entry(*id).or_insert(0.0) += s;
    }

    let mut out: Vec<BlendedCandidate> = by_id
        .into_iter()
        .map(|(sc_track_id, score)| BlendedCandidate { sc_track_id, score })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// Подобрать топ-N кандидатов, не нарушая artist-cap и не повторяя
/// уже виденные в курсоре треки. artist_owner_by_track читается отдельно;
/// если для трека не нашлось мапа — пропускаем (это значит трек ещё не
/// проиндексирован и его не стоит подсовывать).
pub fn pick_with_cap(
    candidates: Vec<BlendedCandidate>,
    artist_by_track: &HashMap<u64, uuid::Uuid>,
    cursor: &WaveCursor,
    limit: usize,
    artist_cap_in_window: usize,
) -> Vec<BlendedCandidate> {
    let mut out: Vec<BlendedCandidate> = Vec::with_capacity(limit);
    let mut local_artist_counts: HashMap<uuid::Uuid, usize> = HashMap::new();
    let mut seen: HashSet<u64> = HashSet::new();
    for c in candidates {
        if out.len() >= limit {
            break;
        }
        if cursor.contains(c.sc_track_id) || !seen.insert(c.sc_track_id) {
            continue;
        }
        if let Some(artist) = artist_by_track.get(&c.sc_track_id) {
            let already = cursor.artist_count_in_window(*artist)
                + local_artist_counts.get(artist).copied().unwrap_or(0);
            if already >= artist_cap_in_window {
                continue;
            }
            local_artist_counts
                .entry(*artist)
                .and_modify(|n| *n += 1)
                .or_insert(1);
        }
        out.push(c);
    }
    out
}

/// Лёгкий collab-arm: top-N треков по user-collab. Если у юзера ещё нет
/// collab-вектора — возвращаем пусто, blender просто не учтёт этот рукав.
pub async fn collab_for_user(
    svc: &RecommendationsService,
    sc_user_id: &str,
    exclude: &HashSet<String>,
    limit: usize,
) -> Vec<(u64, f32)> {
    let Some(vec) = svc.collab.get_user_vector(sc_user_id).await.ok().flatten() else {
        return Vec::new();
    };
    let exclude_vec: Vec<String> = exclude.iter().cloned().collect();
    let filter = svc.build_filter(&exclude_vec, None);
    let raw = svc
        .search_by_vector(
            crate::qdrant::collections::TRACKS_COLLAB,
            &vec,
            filter.as_ref(),
            limit,
        )
        .await;
    raw.into_iter()
        .filter_map(|r| {
            let id =
                crate::modules::recommendations::service::util::value_to_u64(&r.id)?;
            Some((id, r.score.unwrap_or(0.0)))
        })
        .collect()
}
