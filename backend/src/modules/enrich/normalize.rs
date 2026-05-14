use once_cell::sync::Lazy;
use regex::Regex;

static RE_FEAT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(?:feat|ft|featuring)\.?\s+(.+)").unwrap()
});
static RE_PROD: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\bprod(?:uced)?(?:\.|\s+by)?\s+(.+)").unwrap()
});
static RE_REMIX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(.+?)\s+(remix|edit|bootleg|flip|mashup|mix)$").unwrap()
});
static RE_NOISE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(original\s+mix|extended\s+mix|radio\s+edit|free\s+(?:download|dl)|out\s+now|premiere|exclusive|hq|hd|official(?:\s+(?:audio|video))?|lyrics|lyric\s+video|visualizer|hot|new)$").unwrap()
});
static RE_SPLIT_ARTISTS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\s+(?:x|×|vs\.?|&|and|,|feat\.?|ft\.?|featuring)\s+|,\s*").unwrap()
});

/// Срезает префиксы-маркеры роли ("prod. by", "feat.", "remix by" и т.п.),
/// которые могут просочиться в имя артиста из внешних источников (AI, Genius,
/// текст в скобках треков SC). Намеренно НЕ матчит голые слова без явного
/// маркера — иначе зарежет реальные имена вида «Prod Plague» или «With You.»:
///   * `prod`/`produced` — только в связке `by`,
///   * `feat`/`ft`       — только с точкой или в форме `featuring`,
///   * `remix`/`edit`    — только в форме `… by`,
///   * `w/`              — короткая запись «with».
static RE_NAME_PREFIX_NOISE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?ix)
            ^\s*
            (?:
                prod\.?\s+by
              | produced\s+by
              | feat\.
              | featuring
              | ft\.
              | w/
              | remix(?:ed)?\s+by
              | edit(?:ed)?\s+by
            )
            \s+
        ",
    )
    .unwrap()
});

pub fn clean_artist_name(s: &str) -> String {
    let mut cur = s.trim().trim_matches(|c: char| c == '"' || c == '\'').to_string();
    for _ in 0..3 {
        let stripped = RE_NAME_PREFIX_NOISE.replace(&cur, "").to_string();
        if stripped == cur {
            break;
        }
        cur = stripped.trim().to_string();
    }
    cur
}

pub fn normalize_name(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut prev_space = true;
    for c in lower.chars() {
        if c.is_alphanumeric() || c == '&' {
            out.push(c);
            prev_space = false;
        } else if matches!(c, '\'' | '\u{2019}' | '\u{02BC}' | '`') {
            continue;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    let trimmed = out.trim();
    let stripped = trimmed.strip_prefix("the ").unwrap_or(trimmed);
    stripped.to_string()
}

pub fn normalize_title(s: &str) -> String {
    normalize_name(s)
}

/// Дополнительная "плотная" нормализация — alphanumeric only, без пробелов.
/// Используется для сравнения титлов между источниками с разной пунктуацией
/// (например, "1000-7?что ты сказал" vs "1000 - 7что Ты Сказал").
pub fn compact_title(s: &str) -> String {
    normalize_title(s).chars().filter(|c| !c.is_whitespace()).collect()
}

#[derive(Debug, Default, Clone)]
pub struct ParsedTitle {
    pub primary_artists: Vec<String>,
    pub featured: Vec<String>,
    pub producers: Vec<String>,
    pub remixers: Vec<String>,
    pub cleaned_title: String,
}

pub fn parse_sc_title(raw: &str, uploader: Option<&str>) -> ParsedTitle {
    let groups = extract_bracket_groups(raw);
    let stripped = strip_bracket_groups(raw);
    let mut parsed = ParsedTitle::default();

    let (artist_part, title_part) = split_first_dash(&stripped);
    // Префикс вида "01 - …" / "1 - …" — это номер трека в альбоме, а не имя
    // артиста. Отбрасываем "артистную" часть, чтобы fallback ушёл на uploader.
    let artist_part = artist_part.filter(|a| !looks_like_track_number(a));
    let title_clean = title_part.trim().to_string();
    parsed.cleaned_title = if title_clean.is_empty() {
        stripped.trim().to_string()
    } else {
        title_clean
    };

    if let Some(a) = artist_part {
        parsed.primary_artists = split_artists(&a);
    } else if let Some(u) = uploader {
        let u = u.trim();
        if !u.is_empty() {
            parsed.primary_artists.push(u.to_string());
        }
    }

    for g in groups {
        let g = g.trim();
        if g.is_empty() {
            continue;
        }
        if RE_NOISE.is_match(g) {
            continue;
        }
        if let Some(c) = RE_FEAT.captures(g) {
            parsed.featured.extend(split_artists(&c[1]));
            continue;
        }
        if let Some(c) = RE_PROD.captures(g) {
            parsed.producers.extend(split_artists(&c[1]));
            continue;
        }
        if let Some(c) = RE_REMIX.captures(g) {
            let names = split_artists(&c[1]);
            for n in &names {
                parsed.remixers.push(n.clone());
            }
            if !names.is_empty() {
                parsed.cleaned_title = parsed.cleaned_title.trim().to_string();
            }
        }
    }

    dedup_keep_order(&mut parsed.primary_artists);
    dedup_keep_order(&mut parsed.featured);
    dedup_keep_order(&mut parsed.producers);
    dedup_keep_order(&mut parsed.remixers);

    let primary_keys: std::collections::HashSet<String> =
        parsed.primary_artists.iter().map(|s| normalize_name(s)).collect();
    parsed.featured.retain(|s| !primary_keys.contains(&normalize_name(s)));
    parsed.producers.retain(|s| !primary_keys.contains(&normalize_name(s)));
    parsed.remixers.retain(|s| !primary_keys.contains(&normalize_name(s)));

    parsed
}

fn extract_bracket_groups(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth_round = 0i32;
    let mut depth_square = 0i32;
    let mut buf = String::new();
    for c in s.chars() {
        match c {
            '(' => {
                if depth_round == 0 && depth_square == 0 {
                    buf.clear();
                }
                if depth_round > 0 || depth_square > 0 {
                    buf.push(c);
                }
                depth_round += 1;
            }
            ')' => {
                depth_round = (depth_round - 1).max(0);
                if depth_round == 0 && depth_square == 0 {
                    if !buf.is_empty() {
                        out.push(std::mem::take(&mut buf));
                    }
                } else {
                    buf.push(c);
                }
            }
            '[' => {
                if depth_round == 0 && depth_square == 0 {
                    buf.clear();
                }
                if depth_round > 0 || depth_square > 0 {
                    buf.push(c);
                }
                depth_square += 1;
            }
            ']' => {
                depth_square = (depth_square - 1).max(0);
                if depth_round == 0 && depth_square == 0 {
                    if !buf.is_empty() {
                        out.push(std::mem::take(&mut buf));
                    }
                } else {
                    buf.push(c);
                }
            }
            _ => {
                if depth_round > 0 || depth_square > 0 {
                    buf.push(c);
                }
            }
        }
    }
    out
}

fn strip_bracket_groups(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth_round = 0i32;
    let mut depth_square = 0i32;
    for c in s.chars() {
        match c {
            '(' => depth_round += 1,
            ')' => depth_round = (depth_round - 1).max(0),
            '[' => depth_square += 1,
            ']' => depth_square = (depth_square - 1).max(0),
            _ => {
                if depth_round == 0 && depth_square == 0 {
                    out.push(c);
                }
            }
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn looks_like_track_number(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() || t.len() > 3 {
        return false;
    }
    if !t.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    // "01"-"09", "001"-"099" — track-number prefixes. "1"-"99" тоже:
    // голые двузначные числа в начале SC-тайтла практически всегда означают
    // номер. Длина 3 без ведущего нуля ("100"+) — оставляем, чтобы не
    // зарезать реальных артистов «112», «311», «808».
    let n: u32 = t.parse().unwrap_or(u32::MAX);
    t.starts_with('0') || n <= 99
}

fn split_first_dash(s: &str) -> (Option<String>, String) {
    for sep in [" - ", " — ", " – ", " -- "] {
        if let Some(idx) = s.find(sep) {
            let left = s[..idx].trim().to_string();
            let right = s[idx + sep.len()..].trim().to_string();
            if !left.is_empty() {
                return (Some(left), right);
            }
        }
    }
    (None, s.to_string())
}

fn split_artists(s: &str) -> Vec<String> {
    RE_SPLIT_ARTISTS
        .split(s)
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

fn dedup_keep_order(v: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    v.retain(|s| seen.insert(normalize_name(s)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_basic() {
        assert_eq!(normalize_name("Eminem"), "eminem");
        assert_eq!(normalize_name("The Beatles"), "beatles");
        assert_eq!(normalize_name("AC/DC"), "ac dc");
        assert_eq!(normalize_name("Lil Peep"), "lil peep");
        assert_eq!(normalize_name("Lil Peep "), "lil peep");
    }

    #[test]
    fn normalize_unicode() {
        assert_eq!(normalize_name("Эминем"), "эминем");
        assert_eq!(normalize_name("BLACK STAR"), "black star");
    }

    #[test]
    fn normalize_punctuation() {
        // hyphens become spaces — "x-ray" => "x ray". This is intentional.
        assert_eq!(normalize_name("x-ray"), "x ray");
        assert_eq!(normalize_name("Don't Stop"), "dont stop");
    }

    #[test]
    fn compact_title_matches_psychosis_dupe() {
        // Реальный кейс: на SC лежит трек с тайтлом
        //   "Psychosis, Pxlsdead - 1000 - 7что Ты Сказал"
        // Genius даёт wanted с тайтлом
        //   "1000-7?что ты сказал?"
        // parse_sc_title на SC должен отрезать префикс "Psychosis, Pxlsdead - ",
        // оставив cleaned_title = "1000 - 7что Ты Сказал".
        // compact_title обоих результатов должен совпасть.
        let parsed = parse_sc_title("Psychosis, Pxlsdead - 1000 - 7что Ты Сказал", None);
        let cleaned_compact = compact_title(&parsed.cleaned_title);
        let wanted_compact = compact_title("1000-7?что ты сказал?");
        assert_eq!(
            cleaned_compact, wanted_compact,
            "parsed_cleaned={:?} vs wanted={:?}",
            cleaned_compact, wanted_compact
        );
    }

    #[test]
    fn parse_simple_artist_title() {
        let p = parse_sc_title("Eminem - Lose Yourself", None);
        assert_eq!(p.primary_artists, vec!["Eminem"]);
        assert_eq!(p.cleaned_title, "Lose Yourself");
        assert!(p.featured.is_empty());
        assert!(p.remixers.is_empty());
    }

    #[test]
    fn parse_psychosis_x_ray_with_uploader() {
        let p = parse_sc_title("Psychosis - x-ray", Some("louisvuittonkill"));
        assert_eq!(
            p.primary_artists,
            vec!["Psychosis"],
            "primary should be parsed from title, not uploader"
        );
        assert_eq!(p.cleaned_title, "x-ray");
    }

    #[test]
    fn parse_self_upload_no_dash() {
        let p = parse_sc_title("Murder", Some("psychosis"));
        assert_eq!(
            p.primary_artists,
            vec!["psychosis"],
            "no dash → fallback to uploader"
        );
        assert_eq!(p.cleaned_title, "Murder");
    }

    #[test]
    fn parse_feat_in_parens() {
        let p = parse_sc_title("Eminem - Forgot About Dre (feat. Dr. Dre)", None);
        assert_eq!(p.primary_artists, vec!["Eminem"]);
        assert_eq!(p.cleaned_title, "Forgot About Dre");
        assert_eq!(p.featured, vec!["Dr. Dre"]);
    }

    #[test]
    fn parse_multiple_primary_with_x() {
        let p = parse_sc_title("Lil Peep x Lil Tracy - White Tee", None);
        assert_eq!(p.primary_artists, vec!["Lil Peep", "Lil Tracy"]);
        assert_eq!(p.cleaned_title, "White Tee");
    }

    #[test]
    fn parse_remix_in_parens() {
        let p = parse_sc_title("Artist - Track Name (Someone Remix)", None);
        assert_eq!(p.primary_artists, vec!["Artist"]);
        assert_eq!(p.remixers, vec!["Someone"]);
    }

    #[test]
    fn parse_noise_groups_dropped() {
        let p = parse_sc_title("Artist - Track [Free DL] (Original Mix)", None);
        assert_eq!(p.primary_artists, vec!["Artist"]);
        assert!(p.featured.is_empty());
        assert!(p.remixers.is_empty());
    }

    #[test]
    fn parse_em_dash() {
        let p = parse_sc_title("Eminem — Lose Yourself", None);
        assert_eq!(p.primary_artists, vec!["Eminem"]);
        assert_eq!(p.cleaned_title, "Lose Yourself");
    }

    #[test]
    fn parse_no_dash_no_uploader() {
        let p = parse_sc_title("Some Track Name", None);
        assert!(p.primary_artists.is_empty());
        assert_eq!(p.cleaned_title, "Some Track Name");
    }

    #[test]
    fn clean_strips_role_marker_prefixes() {
        assert_eq!(clean_artist_name("prod. by Warykid"), "Warykid");
        assert_eq!(clean_artist_name("prod by Warykid"), "Warykid");
        assert_eq!(clean_artist_name("produced by Warykid"), "Warykid");
        assert_eq!(clean_artist_name("Feat. Warykid"), "Warykid");
        assert_eq!(clean_artist_name("featuring Warykid"), "Warykid");
        assert_eq!(clean_artist_name("ft. Warykid"), "Warykid");
        assert_eq!(clean_artist_name("Remix by Warykid"), "Warykid");
        assert_eq!(clean_artist_name("  \"Warykid\"  "), "Warykid");
    }

    #[test]
    fn clean_keeps_real_names_with_marker_words() {
        // Реальный кейс из БД: артист «Prod Plague» — не должен превратиться
        // в «Plague», потому что нет связки «prod by».
        assert_eq!(clean_artist_name("Prod Plague"), "Prod Plague");
        // Аналогично: трек/имя «With You.» — без `w/` или маркера.
        assert_eq!(clean_artist_name("With You."), "With You.");
        // Голое «ft» / «feat» без точки — оставляем (может быть частью имени).
        assert_eq!(clean_artist_name("ft Warykid"), "ft Warykid");
        assert_eq!(clean_artist_name("Feat Warykid"), "Feat Warykid");
        assert_eq!(clean_artist_name("Warykid"), "Warykid");
    }

    #[test]
    fn parse_track_number_prefix_falls_back_to_uploader() {
        // Реальный кейс: загрузчик «me.xa» льёт альбом с тайтлами вида
        // "02 - Моя Страна Меня Не Любит". Раньше heuristic создавал артиста
        // "02"; теперь "02" опознаётся как номер трека и primary идёт на
        // uploader.
        let p = parse_sc_title("02 - Моя Страна Меня Не Любит", Some("me.xa"));
        assert_eq!(p.primary_artists, vec!["me.xa"]);
        assert_eq!(p.cleaned_title, "Моя Страна Меня Не Любит");

        let p2 = parse_sc_title("1 - Intro", Some("someone"));
        assert_eq!(p2.primary_artists, vec!["someone"]);
        assert_eq!(p2.cleaned_title, "Intro");

        let p3 = parse_sc_title("003 - Outro", Some("someone"));
        assert_eq!(p3.primary_artists, vec!["someone"]);
        assert_eq!(p3.cleaned_title, "Outro");
    }

    #[test]
    fn parse_real_numeric_artist_keeps_name() {
        // Реальный артист «112» (R&B) — 3 цифры без ведущего нуля, оставляем.
        let p = parse_sc_title("112 - Peaches & Cream", None);
        assert_eq!(p.primary_artists, vec!["112"]);
        assert_eq!(p.cleaned_title, "Peaches & Cream");
    }

    #[test]
    fn parse_dedup_primary_in_featured() {
        // "Artist - Title (feat. Artist)" — featured == primary, must dedup
        let p = parse_sc_title("Eminem - Track (feat. Eminem)", None);
        assert_eq!(p.primary_artists, vec!["Eminem"]);
        assert!(p.featured.is_empty());
    }
}

