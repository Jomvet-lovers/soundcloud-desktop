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
