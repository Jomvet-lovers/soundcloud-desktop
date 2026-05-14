//! Канонизация жанро-тегов под выдачу /discover. Хардкодный мап покрывает
//! популярные SC-варианты написания (например, "hip hop" / "hip-hop" / "rap"
//! → "Hip-Hop"). Остальные жанры проходят через title-case как fallback.

pub fn canonicalize_tags(raw: Vec<String>) -> Vec<String> {
    raw.into_iter().filter_map(|t| canonicalize_tag(&t)).collect()
}

pub fn canonicalize_tag(raw: &str) -> Option<String> {
    let lower = raw.trim().to_lowercase();
    if lower.is_empty() {
        return None;
    }
    let canon = match lower.as_str() {
        "hip hop" | "hip-hop" | "hiphop" | "rap" => "Hip-Hop",
        "r&b" | "rnb" | "r and b" | "rhythm and blues" => "R&B",
        "drum and bass" | "drum & bass" | "dnb" | "drumandbass" => "Drum & Bass",
        "electronic" | "electronica" | "edm" => "Electronic",
        "synthwave" | "synth-wave" | "synth wave" => "Synthwave",
        "lo-fi" | "lofi" | "lo fi" => "Lofi",
        "indie" => "Indie",
        "indie pop" => "Indie Pop",
        "indie rock" => "Indie Rock",
        "pop" => "Pop",
        "rock" => "Rock",
        "house" => "House",
        "deep house" => "Deep House",
        "techno" => "Techno",
        "minimal" => "Minimal",
        "ambient" => "Ambient",
        "trap" => "Trap",
        "jazz" => "Jazz",
        "soul" => "Soul",
        "funk" => "Funk",
        "experimental" => "Experimental",
        "shoegaze" => "Shoegaze",
        "post-rock" | "post rock" | "postrock" => "Post-Rock",
        "garage" => "Garage",
        "punk" => "Punk",
        "afrobeat" => "Afrobeat",
        "latin" => "Latin",
        "bossa" | "bossa nova" => "Bossa Nova",
        "cinematic" => "Cinematic",
        "neoclassical" | "neo-classical" => "Neoclassical",
        "industrial" => "Industrial",
        "drone" => "Drone",
        "chillwave" => "Chillwave",
        "surf" => "Surf",
        "idm" => "IDM",
        "dream pop" | "dreampop" => "Dream Pop",
        "synthpop" | "synth-pop" | "synth pop" => "Synthpop",
        "new wave" | "newwave" => "New Wave",
        "bedroom" => "Bedroom",
        "folk" => "Folk",
        "acoustic" => "Acoustic",
        "singer/songwriter" | "singer-songwriter" => "Singer/Songwriter",
        _ => return Some(title_case(&lower)),
    };
    Some(canon.to_string())
}

fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = true;
    for c in s.chars() {
        if c.is_whitespace() || c == '-' {
            out.push(c);
            prev_space = true;
        } else if prev_space {
            for u in c.to_uppercase() {
                out.push(u);
            }
            prev_space = false;
        } else {
            out.push(c);
        }
    }
    out
}
