pub fn normalize_sc_track_id(input: &str) -> Option<String> {
    if input.is_empty() {
        return None;
    }
    let last = if input.contains(':') {
        input.rsplit(':').next().unwrap_or("")
    } else {
        input
    };
    if !last.is_empty() && last.bytes().all(|b| b.is_ascii_digit()) {
        Some(last.to_string())
    } else {
        None
    }
}

/// "soundcloud:tracks:1234" → "1234". Без валидации формата — для случаев
/// когда URN заведомо корректен (приходит из SC ответа / роута).
pub fn extract_sc_id(urn: &str) -> &str {
    urn.rsplit_once(':').map(|(_, id)| id).unwrap_or(urn)
}
