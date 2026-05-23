/// Утилиты для векторов: cosine similarity и L2-нормализация.
/// Жили внутри centroid-сервиса, но сейчас сам сервис не нужен — оставили
/// только функции, к ним обращаются smart_wave/home/similar/artist/collab.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let mut dot = 0f64;
    let mut na = 0f64;
    let mut nb = 0f64;
    for i in 0..n {
        dot += (a[i] as f64) * (b[i] as f64);
        na += (a[i] as f64) * (a[i] as f64);
        nb += (b[i] as f64) * (b[i] as f64);
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom > 0.0 {
        (dot / denom) as f32
    } else {
        0.0
    }
}

pub fn normalize(v: &mut [f32]) {
    let norm = v
        .iter()
        .map(|x| (*x as f64) * (*x as f64))
        .sum::<f64>()
        .sqrt() as f32;
    if norm > 0.0 {
        for x in v {
            *x /= norm;
        }
    }
}
