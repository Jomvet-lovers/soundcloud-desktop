use once_cell::sync::Lazy;
use regex::Regex;

static RE_FEAT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b(?:feat|ft|featuring)\.?\s+(.+)").unwrap());
static RE_PROD: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\bprod(?:uced)?(?:\.|\s+by)?\s+(.+)").unwrap());
static RE_REMIX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^(.+?)\s+(remix|edit|bootleg|flip|mashup|mix)$").unwrap());
static RE_NOISE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(original\s+mix|extended\s+mix|radio\s+edit|free\s+(?:download|dl)|out\s+now|premiere|exclusive|hq|hd|official(?:\s+(?:audio|video))?|lyrics|lyric\s+video|visualizer|hot|new)$").unwrap()
});
/// Сочленители списка артистов. Кириллическая «х» — живой джойнер ру-сцены
/// («Ноггано х Гуф»), как и « + » / unicode-кресты; всё только с пробелами —
/// безпробельный сплит по корпусу ломает имена (Lexxsick, выхухоль).
static RE_SPLIT_ARTISTS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\s+(?:x|х|×|✕|✖|⨯|\+|vs\.?|&|and|w/|feat\.?|ft\.?|featuring)\s+|\s*[,;]\s*")
        .unwrap()
});
/// "03. Aikko - Title" / "03) Aikko" — номер трека в начале artist-части.
/// Срезаем форму с явной точкой/скобкой после числа; голые цифры ловит
/// `looks_like_track_number`. Не трогаем имена вида "M.O.S.T." (там нет
/// ведущих цифр) или "112" / "21 Savage" (нет точки после числа).
static RE_TRACK_NUM_PREFIX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*\d{1,3}\s*[.)]\s+").unwrap());
/// "1.автор" / "01)автор" — номер прилип к имени без пробела. Срез гейтится
/// по uploader'у в parse_sc_title: артист «1.Kla$» не должен терять голову.
/// (Lookahead в крейте regex нет — следующий символ проверяем руками.)
static RE_NUM_PREFIX_TIGHT: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*\d{1,3}[.)]").unwrap());

fn strip_tight_num_prefix(s: &str) -> String {
    if let Some(m) = RE_NUM_PREFIX_TIGHT.find(s) {
        let rest = &s[m.end()..];
        let next_is_name = rest
            .chars()
            .next()
            .map(|c| !c.is_ascii_digit() && !c.is_whitespace() && c != '.' && c != ')')
            .unwrap_or(false);
        if next_is_name {
            return rest.trim().to_string();
        }
    }
    s.to_string()
}
/// "05 Как есть" — номер рипа с ведущим нулём перед словом. По корпусу
/// ведущий ноль = почти гарантированный номер (имена с нуля — единицы).
static RE_LEADING_ZERO_NUM: Lazy<Regex> = Lazy::new(|| Regex::new(r"^0\d{1,2}\s+").unwrap());
/// Анонс-префикс перед настоящим тайтлом: "PREMIERE: A - B", "OUT NOW | …".
static RE_ANNOUNCE_PREFIX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)^\s*(?:world\s+)?(?:premiere|exclusive|out\s+now|free\s+(?:dl|download))\s*[:|]\s*",
    )
    .unwrap()
});
/// `Artist "Track"` / `Артист «Трек»` — кавычки как якорь названия,
/// когда дефиса нет.
static RE_QUOTED_TITLE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"^([^«»"“”\-]{2,60}?)\s+[«"“]([^«»"“”]{2,})[»"”]\s*$"#).unwrap());
/// "Track by Artist" — гейтится по uploader'у; left не должен кончаться
/// кредит-словом ("prod by", "mixed by", "cover by" — это не имя артиста).
static RE_BY_TAIL_CREDIT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(?:prod(?:uced)?|mix(?:ed)?|master(?:ed)?|cover(?:ed)?|remix(?:ed)?|edit(?:ed)?|written|directed)\s*$")
        .unwrap()
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
    let mut cur = s
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'')
        .to_string();
    for _ in 0..3 {
        let stripped = RE_NAME_PREFIX_NOISE.replace(&cur, "").to_string();
        if stripped == cur {
            break;
        }
        cur = stripped.trim().to_string();
    }
    strip_translit_parens(&cur)
}

/// Genius / MB иногда добавляют латинскую транслитерацию к нелатинскому
/// имени в скобках в конце: "МОКЕРИ (moxckery)", "Зейн (zane)". Срезаем
/// если outer содержит non-latin (>U+02AF), а внутри — только латиница.
/// НЕ трогаем role-теги ("трек (cover)", "трек (remix)") и реальные
/// альт-имена вида "Beyoncé (Sasha Fierce)" (обе стороны latin).
pub fn strip_translit_parens(s: &str) -> String {
    let trimmed = s.trim_end();
    if !trimmed.ends_with(')') {
        return s.to_string();
    }
    let Some(open) = trimmed.rfind('(') else {
        return s.to_string();
    };
    let outer = trimmed[..open].trim_end();
    let inner = &trimmed[open + 1..trimmed.len() - 1];
    if outer.is_empty() || inner.is_empty() {
        return s.to_string();
    }
    if looks_like_role_tag(inner) {
        return s.to_string();
    }
    // Не-латиница: всё ВНЕ Basic Latin + Latin-1 + Latin Extended + IPA
    // (≤ U+02AF). "Beyoncé" / "café" — latin-1 с диакритикой, не транслит,
    // НЕ должны триггерить. Кириллица / греческий / CJK / арабский — да.
    let outer_has_nonlatin = outer
        .chars()
        .any(|c| c.is_alphabetic() && (c as u32) > 0x02AF);
    let inner_chars_ok = inner
        .chars()
        .all(|c| c.is_ascii_alphabetic() || matches!(c, ' ' | '-' | '\'' | '.' | '`'));
    let inner_has_letter = inner.chars().any(|c| c.is_ascii_alphabetic());
    if outer_has_nonlatin && inner_chars_ok && inner_has_letter {
        outer.to_string()
    } else {
        s.to_string()
    }
}

/// Содержимое скобок — role/state тег → не трогаем (UI срежет на display
/// после извлечения метаданных enrich-пайплайном).
fn looks_like_role_tag(inner: &str) -> bool {
    let lower = inner.trim().to_lowercase();
    let head = lower.split_whitespace().next().unwrap_or("");
    let tail = lower.split_whitespace().last().unwrap_or("");
    matches!(
        head,
        "cover"
            | "covers"
            | "remix"
            | "rmx"
            | "edit"
            | "version"
            | "mix"
            | "feat"
            | "feat."
            | "ft"
            | "ft."
            | "featuring"
            | "prod"
            | "prod."
            | "produced"
            | "with"
            | "vs"
            | "vs."
            | "instrumental"
            | "acoustic"
            | "live"
            | "demo"
            | "bootleg"
            | "flip"
            | "mashup"
            | "original"
            | "extended"
            | "radio"
            | "free"
            | "official"
            | "premiere"
            | "exclusive"
            | "lyrics"
            | "lyric"
            | "visualizer"
            | "hq"
            | "hd"
    ) || matches!(
        tail,
        "remix"
            | "rmx"
            | "edit"
            | "mix"
            | "version"
            | "cover"
            | "bootleg"
            | "flip"
            | "mashup"
            | "instrumental"
            | "acoustic"
    )
}

/// Канонический match-ключ имени: fold стилизованного юникода и диакритики
/// (см. `artist_names::fold_chars`), lowercase, только буквы/цифры, `&` ≡ "and",
/// без ведущего "the". Этим ключом сравнивается ВСЁ: matcher, persist-дедуп,
/// триаж, `artists.normalized_name`.
pub fn normalize_name(s: &str) -> String {
    let folded = crate::modules::enrich::artist_names::fold_chars(s);
    let mut out = String::with_capacity(folded.len());
    let mut prev_space = true;
    for c in folded.chars() {
        if c == '&' {
            if !prev_space {
                out.push(' ');
            }
            out.push_str("and ");
            prev_space = true;
        } else if c.is_alphanumeric() {
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
    normalize_title(s)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

#[derive(Debug, Default, Clone)]
pub struct ParsedTitle {
    pub primary_artists: Vec<String>,
    pub featured: Vec<String>,
    pub producers: Vec<String>,
    pub remixers: Vec<String>,
    pub cleaned_title: String,
    /// true → в title есть тег `(cover)` / `[cover]`. Resolver не делает
    /// MB/Genius search (иначе подцепит оригинального исполнителя как primary
    /// — а это кавер uploader'а, primary должен остаться = uploader).
    pub is_cover: bool,
    /// true — primary_artists взяты из разметки "Artist - Title" (авторский
    /// сигнал), false — это fallback на uploader или пусто. Resolver по этому
    /// флагу решает, можно ли доверить состав лейбловой мете.
    pub primary_from_title: bool,
    /// Сырая артистная часть разметки (до чисток) — нужна resolver'у, чтобы
    /// откатить перевёрнутый "Track - Artist", когда мета знает правую часть.
    pub raw_artist_part: Option<String>,
}

static RE_COVER: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^\s*cover(\s+version)?\s*$").unwrap());

/// Хвостовое расширение файла — рипы заливают как "трек.mp3".
static RE_FILE_EXT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\s*\.(mp3|m4a|wav|flac|ogg|aac)\s*$").unwrap());

/// Лимит на длину prod-кредита в хвосте — длиннее это уже не ник, а кусок
/// названия, который RE_PROD случайно зацепил.
const TAIL_PROD_MAX: usize = 48;

fn matches_uploader(name: &str, uploader: Option<&str>) -> bool {
    uploader
        .map(|u| crate::modules::enrich::artist_names::same_artist(name, u))
        .unwrap_or(false)
}

pub fn parse_sc_title(raw: &str, uploader: Option<&str>) -> ParsedTitle {
    // Файл-стиль без пробелов: "Lemon_Demon_-_Fine" → "Lemon Demon - Fine".
    let pre = if !raw.contains(' ') && raw.contains('_') {
        raw.replace('_', " ")
    } else {
        raw.to_string()
    };
    // Анонс-обёртка: "PREMIERE: Artist - Track".
    let pre = RE_ANNOUNCE_PREFIX.replace(&pre, "").to_string();

    let groups = extract_bracket_groups(&pre);
    let stripped = strip_bracket_groups(&pre);
    let mut parsed = ParsedTitle::default();

    let (mut artist_part, mut title_part) = split_first_dash(&stripped);
    if artist_part.is_none() {
        // Дефис, прилипший к одной из сторон ("ŦR∀UM∀- Rọtteη", "A -B") или
        // вовсе без пробелов ("Уннв-Без даты"). Режем только когда среди
        // имён левой части есть сам uploader — иначе порвём "x-ray".
        for pat in ["- ", " -", "-"] {
            let Some((l, r)) = split_dash_variant(&stripped, pat) else {
                continue;
            };
            let uploader_in_left = split_artists(&l)
                .iter()
                .any(|p| matches_uploader(p, uploader));
            if uploader_in_left {
                artist_part = Some(l);
                title_part = r;
                break;
            }
        }
    }
    if artist_part.is_none() {
        // `Artist "Track"` / `Артист «Трек»` — кавычки как якорь названия.
        if let Some(c) = RE_QUOTED_TITLE.captures(&stripped) {
            let l = c[1].trim().to_string();
            let r = c[2].trim().to_string();
            if !l.is_empty() && !r.is_empty() {
                artist_part = Some(l);
                title_part = r;
            }
        }
    }
    if artist_part.is_none() {
        // "Track by Artist" — принимаем только когда правая часть = uploader;
        // "… prod by X" / "… mixed by X" — кредит, не разметка.
        if let Some((l, r)) = split_by_keyword(&stripped) {
            if matches_uploader(&r, uploader) {
                artist_part = Some(r);
                title_part = l;
            }
        }
    }

    // Перевёрнутая разметка "Track - Artist": правая часть — это uploader,
    // левая — нет. ("parasite - otuka" от аплоадера otuka.)
    if let Some(a) = artist_part.as_deref() {
        if !title_part.is_empty()
            && !matches_uploader(a, uploader)
            && matches_uploader(&title_part, uploader)
        {
            let new_title = a.to_string();
            artist_part = Some(std::mem::take(&mut title_part));
            title_part = new_title;
        }
    }

    parsed.raw_artist_part = artist_part.clone();
    // Префикс вида "01 - …" / "1 - …" — это номер трека в альбоме, а не имя
    // артиста. Отбрасываем "артистную" часть, чтобы fallback ушёл на uploader.
    // Также режем "03. Aikko" → "Aikko", прилипшее "1.автор" → "автор" и номер
    // рипа с ведущим нулём "05 Имя" → "Имя". Гейт — плотный ключ uploader'а:
    // у "1.Kla$" и "070 Shake" цифры — часть имени, их не трогаем.
    let artist_part = artist_part
        .map(|a| RE_TRACK_NUM_PREFIX.replace(&a, "").trim().to_string())
        .map(|a| {
            let uploader_key = uploader
                .map(crate::modules::enrich::artist_names::compact_key)
                .unwrap_or_default();
            if !uploader_key.is_empty()
                && crate::modules::enrich::artist_names::compact_key(&a) == uploader_key
            {
                a
            } else {
                let a = strip_tight_num_prefix(&a);
                RE_LEADING_ZERO_NUM.replace(&a, "").trim().to_string()
            }
        })
        .filter(|a| !a.is_empty() && !looks_like_track_number(a));
    let title_clean = title_part.trim().to_string();
    parsed.cleaned_title = if title_clean.is_empty() {
        stripped.trim().to_string()
    } else {
        title_clean
    };
    // Номер рипа в начале названия: "05 Как есть" / "Артист - 05 Как есть".
    parsed.cleaned_title = RE_LEADING_ZERO_NUM
        .replace(&parsed.cleaned_title, "")
        .trim()
        .to_string();

    if let Some(a) = artist_part {
        parsed.primary_artists = split_artists(&a);
        parsed.primary_from_title = !parsed.primary_artists.is_empty();
    }
    if parsed.primary_artists.is_empty() {
        parsed.raw_artist_part = None;
        if let Some(u) = uploader {
            let u = u.trim();
            if !u.is_empty() {
                parsed.primary_artists.push(u.to_string());
            }
        }
    }

    for g in groups {
        let g = g.trim();
        if g.is_empty() {
            continue;
        }
        if RE_COVER.is_match(g) {
            parsed.is_cover = true;
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

    parsed.cleaned_title = RE_FILE_EXT
        .replace(&parsed.cleaned_title, "")
        .trim()
        .to_string();

    // prod-кредит без скобок в хвосте: "трек prod. ник" / "трек prod by ник".
    // Каждый десятый feat-тайтл пишет продюсера именно так. Явные маркеры
    // ("prod." / "prod by") принимают многословные ники; голое "prod" — только
    // однословный, чтобы не съесть название со словом produced.
    if let Some(c) = RE_PROD.captures(&parsed.cleaned_title) {
        let m = c.get(0).map(|m| (m.start(), m.end())).unwrap_or((0, 0));
        let marker = c
            .get(0)
            .map(|m| m.as_str())
            .unwrap_or("")
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_lowercase();
        let names = c[1].trim().to_string();
        let explicit = marker.starts_with("prod.") || c[0].to_lowercase().contains(" by ");
        let bare_ok = marker == "prod" && !names.contains(char::is_whitespace);
        if m.0 > 0
            && m.1 == parsed.cleaned_title.len()
            && !names.is_empty()
            && names.chars().count() <= TAIL_PROD_MAX
            && !names.contains(" - ")
            && (explicit || bare_ok)
        {
            parsed.producers.extend(split_artists(&names));
            parsed.cleaned_title = parsed.cleaned_title[..m.0].trim().to_string();
        }
    }

    dedup_keep_order(&mut parsed.primary_artists);
    dedup_keep_order(&mut parsed.featured);
    dedup_keep_order(&mut parsed.producers);
    dedup_keep_order(&mut parsed.remixers);

    let primary_keys: std::collections::HashSet<String> = parsed
        .primary_artists
        .iter()
        .map(|s| normalize_name(s))
        .collect();
    parsed
        .featured
        .retain(|s| !primary_keys.contains(&normalize_name(s)));
    parsed
        .producers
        .retain(|s| !primary_keys.contains(&normalize_name(s)));
    parsed
        .remixers
        .retain(|s| !primary_keys.contains(&normalize_name(s)));

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
    for sep in [" - ", " — ", " – ", " -- ", " ‒ ", " − ", " ─ "] {
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

/// Сплит по дефис-варианту ("- " / " -" / "-"): первое вхождение, "--" мимо.
fn split_dash_variant(s: &str, pat: &str) -> Option<(String, String)> {
    let idx = s.find(pat)?;
    let bytes = s.as_bytes();
    if bytes.get(idx + pat.len()) == Some(&b'-') || (idx > 0 && bytes[idx - 1] == b'-') {
        return None;
    }
    let left = s[..idx].trim();
    let right = s[idx + pat.len()..].trim();
    if left.is_empty() || right.is_empty() {
        return None;
    }
    Some((left.to_string(), right.to_string()))
}

static RE_BY_SPLIT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^(.{2,}?)\s+by\s+(.{2,})$").unwrap());

/// "Track by Artist" → (track, artist); кредит-формы ("prod by …") — мимо.
fn split_by_keyword(s: &str) -> Option<(String, String)> {
    let c = RE_BY_SPLIT.captures(s)?;
    let left = c[1].trim().to_string();
    let right = c[2].trim().to_string();
    if left.is_empty() || right.is_empty() || RE_BY_TAIL_CREDIT.is_match(&left) {
        return None;
    }
    Some((left, right))
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
    fn normalize_folds_stylized_unicode() {
        assert_eq!(normalize_name("ᴍᴏɴᴀʀᴄʜ"), "monarch");
        assert_eq!(normalize_name("𝐕𝐀𝐍𝐓𝐈"), "vanti");
        assert_eq!(normalize_name("Françoise Hardy"), "francoise hardy");
    }

    #[test]
    fn normalize_ampersand_equals_and() {
        assert_eq!(normalize_name("R&B"), "r and b");
        assert_eq!(
            normalize_name("Harold Melvin & The Bluenotes"),
            normalize_name("Harold Melvin and The Bluenotes")
        );
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
    fn parse_track_number_dot_prefix_in_artist_part() {
        // Реальный кейс: трек "03. Aikko - Мне Выгодней Вас Не Знать" — uploader
        // лил альбом и в title зашит номер трека + имя артиста через точку.
        // Должны срезать "03. " и оставить artist = "Aikko".
        let p = parse_sc_title(
            "03. Aikko - Мне Выгодней Вас Не Знать",
            Some("Thirteenth :3"),
        );
        assert_eq!(p.primary_artists, vec!["Aikko"]);
        assert_eq!(p.cleaned_title, "Мне Выгодней Вас Не Знать");

        // После среза остался голый номер — fallback на uploader.
        let p = parse_sc_title("03. 04 - Title", Some("uploader"));
        assert_eq!(p.primary_artists, vec!["uploader"]);

        // "M.O.S.T." — нет ведущих цифр, не трогаем.
        let p = parse_sc_title("M.O.S.T. - Track", None);
        assert_eq!(p.primary_artists, vec!["M.O.S.T."]);
    }

    #[test]
    fn strip_translit_basic() {
        assert_eq!(clean_artist_name("МОКЕРИ (moxckery)"), "МОКЕРИ");
        assert_eq!(clean_artist_name("Зейн (zane)"), "Зейн");
        assert_eq!(clean_artist_name("МОКЕРИ"), "МОКЕРИ");
        // Beyoncé (Sasha Fierce) — оба содержат latin, не транслит → оставляем
        assert_eq!(
            clean_artist_name("Beyoncé (Sasha Fierce)"),
            "Beyoncé (Sasha Fierce)"
        );
        // Только латиница в outer → не трогаем (артист «X (the band)»)
        assert_eq!(clean_artist_name("X (the band)"), "X (the band)");
        // Внутри скобок что-то нелатинское → не транслит → оставляем
        assert_eq!(clean_artist_name("Eminem (Эминем)"), "Eminem (Эминем)");
        // Транслит с пробелом / дефисом — срезаем
        assert_eq!(
            clean_artist_name("Чёрный обелиск (cherny obelisk)"),
            "Чёрный обелиск"
        );
    }

    #[test]
    fn strip_translit_keeps_role_tags() {
        // Role-теги в скобках НЕ трогаем — даже если outer non-latin,
        // а inner чисто латиница (cover/remix/feat/prod/...). Они смысловые,
        // UI-display их срежет; здесь сохраняем чтобы в БД хранился оригинал
        // для последующей обработки enrich-пайплайном.
        assert_eq!(strip_translit_parens("tainted (cover)"), "tainted (cover)");
        assert_eq!(strip_translit_parens("трек (cover)"), "трек (cover)");
        assert_eq!(
            strip_translit_parens("трек (Cover Version)"),
            "трек (Cover Version)"
        );
        assert_eq!(strip_translit_parens("трек (remix)"), "трек (remix)");
        assert_eq!(
            strip_translit_parens("трек (someone remix)"),
            "трек (someone remix)"
        );
        assert_eq!(strip_translit_parens("трек (feat. X)"), "трек (feat. X)");
        assert_eq!(strip_translit_parens("трек (prod. X)"), "трек (prod. X)");
        assert_eq!(
            strip_translit_parens("трек (instrumental)"),
            "трек (instrumental)"
        );
        assert_eq!(strip_translit_parens("трек (live)"), "трек (live)");
        // А вот translit без role-тега — срезаем (имя в скобках, не роль)
        assert_eq!(strip_translit_parens("трек (translit)"), "трек");
    }

    #[test]
    fn parse_dedup_primary_in_featured() {
        // "Artist - Title (feat. Artist)" — featured == primary, must dedup
        let p = parse_sc_title("Eminem - Track (feat. Eminem)", None);
        assert_eq!(p.primary_artists, vec!["Eminem"]);
        assert!(p.featured.is_empty());
    }

    // ─── паттерны из корпуса прода (см. /tmp/title_patterns_*.md) ───

    #[test]
    fn parse_cyrillic_x_joiner() {
        // "1 х 2 х 3 х 4 - трек" — кириллическая х как сочленитель.
        let p = parse_sc_title("Ноггано х Гуф х АК-47 - Тем Кто С Нами", None);
        assert_eq!(p.primary_artists, vec!["Ноггано", "Гуф", "АК-47"]);
        assert_eq!(p.cleaned_title, "Тем Кто С Нами");

        let p2 = parse_sc_title("SALUKI + 104 - ЗИМА", None);
        assert_eq!(p2.primary_artists, vec!["SALUKI", "104"]);
    }

    #[test]
    fn parse_numeric_alias_artists_spaced_chain() {
        // Числовые алиасы — реальные артисты; со спейсами режем смело.
        let p = parse_sc_title("1 х 2 х 3 х 4 - трек", None);
        assert_eq!(p.primary_artists, vec!["1", "2", "3", "4"]);
        assert_eq!(p.cleaned_title, "трек");
    }

    #[test]
    fn parse_unspaced_chain_not_split() {
        // Безпробельную склейку НЕ режем (по корпусу это почти всегда имя:
        // Lexxsick, выхухоль). Раскрытие через мету делает resolver.
        let p = parse_sc_title("1х2х3х4х5 - трек", None);
        assert_eq!(p.primary_artists, vec!["1х2х3х4х5"]);
    }

    #[test]
    fn parse_numbered_list_with_feat() {
        // "1. автор, фит - трек"
        let p = parse_sc_title("1. uglystephan, lil heaven - не различаю", None);
        assert_eq!(p.primary_artists, vec!["uglystephan", "lil heaven"]);
        assert_eq!(p.cleaned_title, "не различаю");
    }

    #[test]
    fn parse_tight_number_prefix() {
        // "1.автор-трек" — номер прилип к имени, дефис без пробелов.
        let p = parse_sc_title("1.uglystephan-не различаю", Some("uglystephan"));
        assert_eq!(p.primary_artists, vec!["uglystephan"]);
        assert_eq!(p.cleaned_title, "не различаю");

        // Прилипший номер + обычный дефис.
        let p2 = parse_sc_title("03.Aikko - Песня", Some("кто-то"));
        assert_eq!(p2.primary_artists, vec!["Aikko"]);
    }

    #[test]
    fn parse_number_in_artist_name_kept() {
        // "1.Kla$" / "070 Shake" — цифры часть имени, гейт по uploader'у.
        let p = parse_sc_title("1.Kla$ - Russisch", Some("1.Kla$"));
        assert_eq!(p.primary_artists, vec!["1.Kla$"]);

        let p2 = parse_sc_title("070 Shake - Guilty Conscience", Some("070 Shake"));
        assert_eq!(p2.primary_artists, vec!["070 Shake"]);
    }

    #[test]
    fn parse_leading_zero_rip_number() {
        // Без матча с uploader'ом ведущий ноль — номер рипа.
        let p = parse_sc_title("05 Как есть - demo", Some("кто-то"));
        assert_eq!(p.primary_artists, vec!["Как есть"]);
        // И в названии после дефиса.
        let p2 = parse_sc_title("Захар - 05 Как есть", None);
        assert_eq!(p2.cleaned_title, "Как есть");
    }

    #[test]
    fn parse_bare_dash_with_uploader_gate() {
        // "Уннв-Без даты" от самого УННВ — режем; "x-ray" чужое — нет.
        let p = parse_sc_title("Уннв-Без даты", Some("УННВ"));
        assert_eq!(p.primary_artists, vec!["Уннв"]);
        assert_eq!(p.cleaned_title, "Без даты");

        let p2 = parse_sc_title("x-ray", Some("psychosis"));
        assert_eq!(p2.primary_artists, vec!["psychosis"]);
        assert_eq!(p2.cleaned_title, "x-ray");
    }

    #[test]
    fn parse_reversed_markup_by_uploader() {
        // "parasite - otuka" от аплоадера otuka → артист справа.
        let p = parse_sc_title("parasite - otuka", Some("otuka"));
        assert_eq!(p.primary_artists, vec!["otuka"]);
        assert_eq!(p.cleaned_title, "parasite");
    }

    #[test]
    fn parse_quoted_title_anchor() {
        let p = parse_sc_title("Скриптонит «Положение»", None);
        assert_eq!(p.primary_artists, vec!["Скриптонит"]);
        assert_eq!(p.cleaned_title, "Положение");

        let p2 = parse_sc_title("Juice WRLD \"Lucid Dreams\"", None);
        assert_eq!(p2.primary_artists, vec!["Juice WRLD"]);
        assert_eq!(p2.cleaned_title, "Lucid Dreams");
    }

    #[test]
    fn parse_track_by_artist_with_uploader_gate() {
        let p = parse_sc_title("Lucid Dreams by Juice WRLD", Some("Juice WRLD"));
        assert_eq!(p.primary_artists, vec!["Juice WRLD"]);
        assert_eq!(p.cleaned_title, "Lucid Dreams");

        // "prod by X" — кредит, не разметка.
        let p2 = parse_sc_title("hard one prod by warykid", Some("warykid"));
        assert!(p2.cleaned_title.starts_with("hard one"));
    }

    #[test]
    fn parse_underscore_file_style() {
        let p = parse_sc_title("Lemon_Demon_-_Fine", None);
        assert_eq!(p.primary_artists, vec!["Lemon Demon"]);
        assert_eq!(p.cleaned_title, "Fine");
    }

    #[test]
    fn parse_announce_prefix_stripped() {
        let p = parse_sc_title("PREMIERE: Boys Noize - Mvinline", None);
        assert_eq!(p.primary_artists, vec!["Boys Noize"]);
        assert_eq!(p.cleaned_title, "Mvinline");
    }

    #[test]
    fn parse_file_extension_stripped() {
        let p = parse_sc_title("Дора - Дорадура.mp3", None);
        assert_eq!(p.cleaned_title, "Дорадура");
    }

    #[test]
    fn parse_tail_prod_credit() {
        // prod-кредит без скобок в хвосте → producers, из названия вон.
        let p = parse_sc_title("Doc Rivers prod. RichRo", None);
        assert_eq!(p.cleaned_title, "Doc Rivers");
        assert_eq!(p.producers, vec!["RichRo"]);

        let p2 = parse_sc_title("go up! drowning prod level", None);
        assert_eq!(p2.cleaned_title, "go up! drowning");
        assert_eq!(p2.producers, vec!["level"]);

        // Голое "prod" с многословным хвостом — не трогаем (это название).
        let p3 = parse_sc_title("we produced this together", None);
        assert_eq!(p3.cleaned_title, "we produced this together");
        assert!(p3.producers.is_empty());
    }

    #[test]
    fn parse_exotic_dashes() {
        let p = parse_sc_title("Artist ‒ Track", None);
        assert_eq!(p.primary_artists, vec!["Artist"]);
        assert_eq!(p.cleaned_title, "Track");
    }

    #[test]
    fn parse_glued_dash_with_uploader_in_left_list() {
        // Реальный лайк: "LifeelBeatsVNZL & ŦR∀UM∀- Rọtteη Bløød" от ŦR∀UM∀ —
        // дефис прилип к имени, uploader лишь один из перечисленных.
        let p = parse_sc_title("LifeelBeatsVNZL & ŦR∀UM∀- Rọtteη Bløød", Some("ŦR∀UM∀"));
        assert_eq!(p.primary_artists, vec!["LifeelBeatsVNZL", "ŦR∀UM∀"]);
        assert_eq!(p.cleaned_title, "Rọtteη Bløød");

        // Прилипший справа.
        let p2 = parse_sc_title("Уннв -Дворы", Some("УННВ"));
        assert_eq!(p2.primary_artists, vec!["Уннв"]);
        assert_eq!(p2.cleaned_title, "Дворы");

        // Без uploader-матча не трогаем.
        let p3 = parse_sc_title("self- titled", Some("кто-то"));
        assert!(p3.cleaned_title.contains("self"));
    }
}
