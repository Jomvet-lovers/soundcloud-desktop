use std::collections::BTreeSet;

use once_cell::sync::Lazy;
use regex::Regex;

static RE_FEAT_PAREN: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\(feat\.?[^)]*\)").unwrap());
static RE_FT_PAREN: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\(ft\.?[^)]*\)").unwrap());
static RE_BRACKETS: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[[^\]]*\]").unwrap());
static RE_VARIANT_PAREN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\([^)]*?(remix|edit|version|mix|cover|live|acoustic|instrumental|original|prod)[^)]*?\)")
        .unwrap()
});
static RE_FEAT_TAIL: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\s+(feat\.?|ft\.?|featuring|prod\.?)\b.*$").unwrap());
static RE_PARENS: Lazy<Regex> = Lazy::new(|| Regex::new(r"\([^)]*\)").unwrap());
static RE_WS: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());
static RE_LRC_LINE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\[(\d{2}):(\d{2})\.(\d{2,3})\]\s*(.*)").unwrap());
static RE_LRC_TS: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[\d{2}:\d{2}\.\d{2,3}\]").unwrap());

pub fn clean_title(s: &str) -> String {
    let mut out = RE_FEAT_PAREN.replace_all(s, "").into_owned();
    out = RE_FT_PAREN.replace_all(&out, "").into_owned();
    out = RE_BRACKETS.replace_all(&out, "").into_owned();
    out = RE_VARIANT_PAREN.replace_all(&out, "").into_owned();
    out = RE_FEAT_TAIL.replace_all(&out, "").into_owned();
    out = RE_WS.replace_all(&out, " ").into_owned();
    out.trim().to_string()
}

pub fn strip_brackets(s: &str) -> String {
    let mut out = RE_PARENS.replace_all(s, "").into_owned();
    out = RE_BRACKETS.replace_all(&out, "").into_owned();
    out = RE_WS.replace_all(&out, " ").into_owned();
    out.trim().to_string()
}

pub fn alpha_only(s: &str) -> String {
    let mut buf = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_alphanumeric() || ch.is_whitespace() {
            buf.push(ch);
        }
    }
    RE_WS.replace_all(&buf, " ").trim().to_string()
}

pub fn canon_meta(s: &str) -> String {
    alpha_only(&strip_brackets(&clean_title(s))).to_lowercase()
}

pub fn split_artist_title(raw: &str) -> Option<(String, String)> {
    for sep in [" - ", " – ", " — ", " // "] {
        if let Some(idx) = raw.find(sep) {
            if idx > 0 {
                let artist = raw[..idx].trim().to_string();
                let title = raw[idx + sep.len()..].trim().to_string();
                if !artist.is_empty() && !title.is_empty() {
                    return Some((artist, title));
                }
            }
        }
    }
    None
}

#[derive(Debug, Clone)]
pub struct LyricLine {
    pub time: f64,
    pub text: String,
}

pub fn parse_lrc(lrc: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    for raw in lrc.split('\n') {
        let Some(caps) = RE_LRC_LINE.captures(raw) else { continue };
        let mm: f64 = caps[1].parse().unwrap_or(0.0);
        let ss: f64 = caps[2].parse().unwrap_or(0.0);
        let mut frac_str = caps[3].to_string();
        while frac_str.len() < 3 {
            frac_str.push('0');
        }
        let ms: f64 = frac_str.parse().unwrap_or(0.0);
        let time = mm * 60.0 + ss + ms / 1000.0;
        let text = caps[4].trim().to_string();
        if !text.is_empty() {
            lines.push(LyricLine { time, text });
        }
    }
    lines
}

pub fn strip_lrc_timestamps(lrc: &str) -> String {
    let mut out = Vec::new();
    for line in lrc.split('\n') {
        let stripped = RE_LRC_TS.replace_all(line, "");
        let trimmed = stripped.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
    }
    out.join("\n")
}

pub fn pick_lyrics_text(plain: Option<&str>, synced: Option<&str>) -> Option<String> {
    let p = plain.unwrap_or("").trim();
    if !p.is_empty() {
        return Some(p.to_string());
    }
    let s = synced.unwrap_or("").trim();
    if s.is_empty() {
        return None;
    }
    let stripped = strip_lrc_timestamps(s);
    let trimmed = stripped.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub struct LangDetect {
    pub language: String,
    pub confidence: f32,
}

pub fn detect_language_heuristic(text: &str) -> Option<LangDetect> {
    if text.is_empty() {
        return None;
    }
    let mut counts: std::collections::HashMap<&'static str, usize> = std::collections::HashMap::new();
    let mut total: usize = 0;
    let sample: String = text.chars().take(4000).collect();
    for ch in sample.chars() {
        let cp = ch as u32;
        let script: Option<&'static str> = if (0x0400..=0x04ff).contains(&cp) {
            Some("cyrillic")
        } else if (0x0370..=0x03ff).contains(&cp) {
            Some("greek")
        } else if (0x0590..=0x05ff).contains(&cp) {
            Some("hebrew")
        } else if (0x0600..=0x06ff).contains(&cp) {
            Some("arabic")
        } else if (0xac00..=0xd7af).contains(&cp) {
            Some("hangul")
        } else if (0x3040..=0x309f).contains(&cp) {
            Some("hiragana")
        } else if (0x30a0..=0x30ff).contains(&cp) {
            Some("katakana")
        } else if (0x4e00..=0x9fff).contains(&cp) {
            Some("cjk")
        } else if (0x41..=0x5a).contains(&cp) || (0x61..=0x7a).contains(&cp) {
            Some("latin")
        } else {
            None
        };
        if let Some(s) = script {
            *counts.entry(s).or_insert(0) += 1;
            total += 1;
        }
    }
    if total < 20 {
        return None;
    }
    let mut best_script = "latin";
    let mut best_count = 0usize;
    for (k, v) in &counts {
        if *v > best_count {
            best_count = *v;
            best_script = *k;
        }
    }
    let ratio = best_count as f32 / total as f32;
    if ratio < 0.3 {
        return None;
    }
    let language = match best_script {
        "cyrillic" => "ru",
        "hangul" => "ko",
        "hiragana" | "katakana" => "ja",
        "cjk" => {
            let ja = counts.get("hiragana").copied().unwrap_or(0)
                + counts.get("katakana").copied().unwrap_or(0);
            if ja > 0 { "ja" } else { "zh" }
        }
        "arabic" => "ar",
        "hebrew" => "he",
        "greek" => "el",
        _ => "en",
    };
    Some(LangDetect {
        language: language.to_string(),
        confidence: ratio,
    })
}

pub fn heuristic_queries(artist: &str, title: &str) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    let mut order: Vec<String> = Vec::new();
    let mut add = |s: &str, order: &mut Vec<String>, out: &mut BTreeSet<String>| {
        let cleaned = RE_WS.replace_all(&alpha_only(s), " ").trim().to_string();
        if cleaned.len() >= 2 && !out.contains(&cleaned) {
            out.insert(cleaned.clone());
            order.push(cleaned);
        }
    };

    let clean_t = clean_title(title);
    let stripped_t = strip_brackets(title);

    add(&format!("{artist} {title}"), &mut order, &mut out);
    add(&format!("{artist} {clean_t}"), &mut order, &mut out);
    add(&format!("{artist} {stripped_t}"), &mut order, &mut out);

    if let Some((real_artist, real_title)) = split_artist_title(title) {
        let real_title_clean = clean_title(&real_title);
        add(&format!("{real_artist} {real_title}"), &mut order, &mut out);
        add(&format!("{real_artist} {real_title_clean}"), &mut order, &mut out);
        add(&real_title_clean, &mut order, &mut out);
    }

    add(&clean_t, &mut order, &mut out);
    add(&stripped_t, &mut order, &mut out);

    order.into_iter().take(6).collect()
}
