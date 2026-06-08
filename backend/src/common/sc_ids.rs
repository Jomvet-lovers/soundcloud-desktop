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

/// SoundCloud user-id живёт в двух формах — URN (`soundcloud:users:123`) и
/// голой (`123`). На проде строки исторически расщеплены по обеим; чтобы
/// per-user запрос видел ВСЕ строки юзера до канонизации (миграция 0043),
/// матчим по обоим вариантам (`user_id = ANY(...)`). Канон записи — bare.
pub fn user_id_variants(sc_user_id: &str) -> Vec<String> {
    let trimmed = sc_user_id.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let bare = trimmed.rsplit(':').next().unwrap_or(trimmed);
    let mut out = vec![trimmed.to_string()];
    if bare != trimmed {
        out.push(bare.to_string());
    }
    if bare.bytes().all(|b| b.is_ascii_digit()) && !bare.is_empty() {
        let urn = format!("soundcloud:users:{bare}");
        if urn != trimmed {
            out.push(urn);
        }
    }
    out.dedup();
    out
}
