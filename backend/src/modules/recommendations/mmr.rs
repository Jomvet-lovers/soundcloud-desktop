use crate::modules::centroids::cosine;

/// Greedy MMR-style selection: pick `limit` items from a pool of size
/// `pool_vecs.len()`, each step picking the index that maximises
/// `score(cand_idx, already_selected_indices, pool_vecs)`.
///
/// Callers compose relevance / diversity / novelty / serendipity themselves
/// inside `score` — this helper only owns the greedy loop.
///
/// Returns selected indices in selection order.
pub fn greedy_pick<F>(pool_vecs: &[Vec<f32>], limit: usize, mut score: F) -> Vec<usize>
where
    F: FnMut(usize, &[usize], &[Vec<f32>]) -> f32,
{
    let want = limit.min(pool_vecs.len());
    if want == 0 {
        return Vec::new();
    }

    let mut selected: Vec<usize> = Vec::with_capacity(want);
    let mut taken = vec![false; pool_vecs.len()];

    while selected.len() < want {
        let mut best_idx = usize::MAX;
        let mut best_val = f32::NEG_INFINITY;
        for (i, &t) in taken.iter().enumerate() {
            if t {
                continue;
            }
            let s = score(i, &selected, pool_vecs);
            if s > best_val {
                best_val = s;
                best_idx = i;
            }
        }
        if best_idx == usize::MAX {
            break;
        }
        taken[best_idx] = true;
        selected.push(best_idx);
    }
    selected
}

/// Max cosine similarity between `pool_vecs[cand]` and any vector indexed by
/// `selected`. Returns 0.0 when `selected` is empty.
pub fn max_cosine_to_selected(cand: usize, selected: &[usize], pool_vecs: &[Vec<f32>]) -> f32 {
    let mut m: f32 = 0.0;
    for &si in selected {
        let v = cosine(&pool_vecs[cand], &pool_vecs[si]);
        if v > m {
            m = v;
        }
    }
    m
}
