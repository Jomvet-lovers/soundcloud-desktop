use super::service::RecommendResult;

pub fn ips_debias(items: &mut [RecommendResult]) {
    for it in items.iter_mut() {
        let plays = it.playback_count.unwrap_or(0).max(0) as f64;
        let denom = (1.0 + plays.ln_1p()).sqrt() as f32;
        if denom > 1.0 {
            if let Some(s) = it.score.as_mut() {
                *s /= denom;
            }
        }
    }
    items.sort_by(|a, b| {
        b.score
            .unwrap_or(0.0)
            .partial_cmp(&a.score.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}
