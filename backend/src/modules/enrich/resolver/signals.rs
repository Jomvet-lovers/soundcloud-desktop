//! Локальные сигналы трека — всё, что известно без внешних API: разметка
//! заголовка, лейбловая мета, uploader. Считаются ОДИН раз на трек и
//! используются каскадом, verified fast-path'ом и AI-проверкой одинаково.

use std::collections::HashSet;

use tracing::debug;

use crate::error::AppResult;
use crate::modules::enrich::artist_names;
use crate::modules::enrich::normalize::{normalize_name, parse_sc_title, ParsedTitle};

use super::{ArtistCandidate, ResolveResult, ResolveSource, TrackContext};

pub struct LocalSignals {
    /// Разметка заголовка после unreverse и словарной сегментации.
    pub parsed: ParsedTitle,
    /// Имена из `metadata_artist` (unescape + split + джанк-фильтр).
    pub meta_names: Vec<String>,
}

impl LocalSignals {
    /// Полный пролог: parse → unreverse по мете → словарная сегментация по
    /// каталогу artists. Ошибка словаря не фатальна (сегментация — бонус).
    pub async fn build(ctx: &TrackContext, pg: &sqlx::PgPool) -> Self {
        let mut signals = Self::parse(ctx);
        match fetch_segment_dictionary(&signals.parsed, pg).await {
            Ok(dict) => segment_by_dictionary(&mut signals.parsed, &dict),
            Err(e) => debug!(error = %e, "dictionary fetch failed"),
        }
        signals
    }

    /// Тот же пролог с готовым словарём — для тестов (без PG).
    #[cfg(test)]
    pub fn build_with_dictionary(ctx: &TrackContext, dict: &HashSet<String>) -> Self {
        let mut signals = Self::parse(ctx);
        segment_by_dictionary(&mut signals.parsed, dict);
        signals
    }

    fn parse(ctx: &TrackContext) -> Self {
        let mut parsed = parse_sc_title(&ctx.title, ctx.uploader_username.as_deref());
        let meta_names = ctx
            .metadata_artist
            .as_deref()
            .map(artist_names::meta_artist_names)
            .unwrap_or_default();
        maybe_unreverse_with_meta(&mut parsed, &meta_names);
        Self { parsed, meta_names }
    }

    /// Имена из авторской разметки заголовка (не uploader-fallback).
    pub fn markup(&self) -> Option<&[String]> {
        if self.parsed.primary_from_title && !self.parsed.primary_artists.is_empty() {
            Some(&self.parsed.primary_artists)
        } else {
            None
        }
    }

    /// Лучшая локальная догадка об артисте для внешнего поиска.
    pub fn primary_hint(&self, ctx: &TrackContext) -> Option<String> {
        self.parsed
            .primary_artists
            .first()
            .filter(|_| self.parsed.primary_from_title)
            .cloned()
            .or_else(|| self.meta_names.first().cloned())
            .or_else(|| ctx.uploader_username.clone())
    }

    /// Очищенное название для внешнего поиска (fallback — сырой заголовок).
    pub fn title_query(&self, ctx: &TrackContext) -> String {
        if self.parsed.cleaned_title.is_empty() {
            ctx.title.clone()
        } else {
            self.parsed.cleaned_title.clone()
        }
    }

    /// Локальный резолв без внешних API. Иерархия доверия:
    ///   1. явная разметка "Artist - Title" в заголовке (авторская),
    ///   2. лейбловая `metadata_artist` (дистрибьюторская, уже без мусора),
    ///   3. загрузчик.
    ///
    /// Мета и разметка согласны → объединяем составы (мета знает co-артистов,
    /// которых в заголовке поленились перечислить, и наоборот).
    pub fn heuristic(&self, ctx: &TrackContext) -> ResolveResult {
        let parsed = &self.parsed;
        let meta_names = &self.meta_names;
        let to_candidate = |name: &str, sc_user_id: Option<String>| ArtistCandidate {
            name: name.to_string(),
            mb_id: None,
            genius_id: None,
            sc_user_id,
        };
        let attach_uploader_sc = |n: &str| {
            if name_matches_uploader(n, ctx.uploader_username.as_deref()) {
                ctx.uploader_sc_user_id.clone()
            } else {
                None
            }
        };

        let mut source = ResolveSource::Heuristic;
        let mut primary: Vec<ArtistCandidate> = if parsed.primary_from_title {
            parsed
                .primary_artists
                .iter()
                .map(|n| to_candidate(n, attach_uploader_sc(n)))
                .collect()
        } else {
            Vec::new()
        };

        if primary.is_empty() {
            // Мета бывает кредитным списком: "RAMPAGE (PROD. X)" с метой "X" —
            // имена, уже распознанные парсером в другие роли, в primary не берём.
            let credited: Vec<&str> = parsed
                .featured
                .iter()
                .chain(parsed.producers.iter())
                .chain(parsed.remixers.iter())
                .map(|s| s.as_str())
                .collect();
            let meta_primaries: Vec<&String> = meta_names
                .iter()
                .filter(|m| !artist_names::name_in(m, credited.iter().copied()))
                .collect();
            if !meta_primaries.is_empty() {
                source = ResolveSource::Meta;
                primary = meta_primaries
                    .iter()
                    .map(|n| to_candidate(n, attach_uploader_sc(n)))
                    .collect();
            } else if let Some(u) = ctx.uploader_username.as_deref() {
                primary.push(to_candidate(u, ctx.uploader_sc_user_id.clone()));
            }
        } else if !meta_names.is_empty() {
            let title_names: Vec<&str> =
                parsed.primary_artists.iter().map(|s| s.as_str()).collect();
            // Сначала склейка: "ALUCIFYxBACKW666S - трек" при мете
            // "alucify, backw666s" дословно равна мете — раскрываем по ней.
            // (Проверка идёт до agree-union: substring-похожесть считает кусок
            // склейки «тем же артистом» и уводит в неверную ветку.)
            if let Some(chain) = unspaced_chain_matches_meta(&title_names, meta_names) {
                source = ResolveSource::Meta;
                primary = chain
                    .iter()
                    .map(|n| to_candidate(n, attach_uploader_sc(n)))
                    .collect();
            } else {
                let agrees = meta_names
                    .iter()
                    .any(|m| artist_names::name_in(m, title_names.iter().copied()));
                if agrees {
                    source = ResolveSource::Meta;
                    let known: Vec<&str> = parsed
                        .primary_artists
                        .iter()
                        .chain(parsed.featured.iter())
                        .chain(parsed.producers.iter())
                        .chain(parsed.remixers.iter())
                        .map(|s| s.as_str())
                        .collect();
                    for m in meta_names {
                        if !artist_names::name_in(m, known.iter().copied()) {
                            primary.push(to_candidate(m, attach_uploader_sc(m)));
                        }
                    }
                }
            }
        }

        let featured = parsed
            .featured
            .iter()
            .map(|n| to_candidate(n, None))
            .collect();
        let producers = parsed
            .producers
            .iter()
            .map(|n| to_candidate(n, None))
            .collect();
        let remixers = parsed
            .remixers
            .iter()
            .map(|n| to_candidate(n, None))
            .collect();

        let self_upload = primary.iter().any(|p| p.sc_user_id.is_some());
        let confidence = if primary.is_empty() {
            0.05
        } else {
            match (source, self_upload) {
                (ResolveSource::Meta, true) => 0.65,
                (ResolveSource::Meta, false) => 0.5,
                (_, true) => 0.55,
                (_, false) => 0.2,
            }
        };
        ResolveResult {
            source,
            confidence,
            primary,
            featured,
            producers,
            remixers,
            album: None,
            isrc: ctx.isrc.clone(),
            release_date: None,
            release_year: None,
            is_cover: parsed.is_cover,
            degraded: false,
        }
    }
}

pub(super) fn name_matches_uploader(name: &str, uploader: Option<&str>) -> bool {
    let Some(u) = uploader else { return false };
    normalize_name(name) == normalize_name(u)
}

/// Перевёрнутая разметка "Track - Artist" (~4% дефисных тайтлов по корпусу):
/// левая часть мете неизвестна, а правая — ровно артист из меты. Откатываем:
/// "505 - arctic monkeys" + мета "Arctic Monkeys" → артист справа.
fn maybe_unreverse_with_meta(parsed: &mut ParsedTitle, meta_names: &[String]) {
    if !parsed.primary_from_title || meta_names.is_empty() || parsed.cleaned_title.is_empty() {
        return;
    }
    let meta_strs = || meta_names.iter().map(|s| s.as_str());
    let left_known = parsed
        .primary_artists
        .iter()
        .any(|t| artist_names::name_in(t, meta_strs()));
    if left_known || !artist_names::name_in(&parsed.cleaned_title, meta_strs()) {
        return;
    }
    let new_title = parsed
        .raw_artist_part
        .take()
        .unwrap_or_else(|| parsed.primary_artists.join(", "));
    parsed.primary_artists = vec![std::mem::replace(&mut parsed.cleaned_title, new_title)];
}

/// Один title-токен, плотная склейка которого равна склейке ВСЕХ имён меты
/// (просто подряд или через x-джойнер) → вернуть мету как состав.
fn unspaced_chain_matches_meta<'a>(
    title_names: &[&str],
    meta_names: &'a [String],
) -> Option<&'a [String]> {
    if title_names.len() != 1 || meta_names.len() < 2 {
        return None;
    }
    let left = artist_names::compact_key(title_names[0]);
    if left.is_empty() {
        return None;
    }
    let keys: Vec<String> = meta_names
        .iter()
        .map(|m| artist_names::compact_key(m))
        .collect();
    if keys.iter().any(|k| k.is_empty()) {
        return None;
    }
    let plain = keys.concat();
    // Джойнер бывает латинским и кириллическим: AxB / AхB.
    if left == plain || left == keys.join("x") || left == keys.join("х") {
        Some(meta_names)
    } else {
        None
    }
}

/// Ключи каталога для сегментации: нормализованные склейки всех под-отрезков
/// 1..=N слов кандидата (короче 3 символов — шум, мимо).
fn segment_keys(words: &[&str]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for i in 0..words.len() {
        for j in i + 1..=words.len() {
            let key = normalize_name(&words[i..j].join(" "));
            if key.chars().count() >= 3 && !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    keys
}

/// Кандидат на сегментацию: единственное имя из разметки, 2..=6 слов.
fn segmentation_candidate(parsed: &ParsedTitle) -> Option<String> {
    if !parsed.primary_from_title || parsed.primary_artists.len() != 1 {
        return None;
    }
    let name = &parsed.primary_artists[0];
    (2..=6)
        .contains(&name.split_whitespace().count())
        .then(|| name.clone())
}

/// Существующие в каталоге ключи под-отрезков кандидата. Только доверенные
/// артисты (внешний источник или высокий confidence) — иначе словарь из
/// heuristic-мусора ("Intro", "Demo") шинкует настоящие имена.
async fn fetch_segment_dictionary(
    parsed: &ParsedTitle,
    pg: &sqlx::PgPool,
) -> AppResult<HashSet<String>> {
    let Some(name) = segmentation_candidate(parsed) else {
        return Ok(HashSet::new());
    };
    let words: Vec<&str> = name.split_whitespace().collect();
    let keys = segment_keys(&words);
    if keys.is_empty() {
        return Ok(HashSet::new());
    }
    let existing =
        sqlx::query_file_scalar!("queries/enrich/service/artists_by_normalized.sql", &keys)
            .fetch_all(pg)
            .await?;
    Ok(existing.into_iter().collect())
}

/// Сегментация артистной части, склеенной пробелами без сочленителей:
/// "Aikko Own Maslou - Трек" → [Aikko, Own Maslou] — но только если КАЖДЫЙ
/// сегмент существует в каталоге artists, а склейка целиком — нет (иначе это
/// цельное имя вида "Lil Peep"). Greedy по длиннейшему совпадению слева.
fn segment_by_dictionary(parsed: &mut ParsedTitle, dict: &HashSet<String>) {
    if dict.is_empty() {
        return;
    }
    let Some(name) = segmentation_candidate(parsed) else {
        return;
    };
    let words: Vec<&str> = name.split_whitespace().collect();
    if dict.contains(&normalize_name(&name)) {
        return;
    }

    let mut segments: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let mut matched_to = 0;
        for j in (i + 1..=words.len()).rev() {
            let key = normalize_name(&words[i..j].join(" "));
            if key.chars().count() >= 3 && dict.contains(&key) {
                matched_to = j;
                break;
            }
        }
        if matched_to == 0 {
            return;
        }
        segments.push(words[i..matched_to].join(" "));
        i = matched_to;
    }
    if segments.len() >= 2 {
        debug!(?segments, original = %name, "dictionary segmentation applied");
        parsed.primary_artists = segments;
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{ctx, names, run_heuristic};
    use super::*;
    use crate::modules::enrich::normalize::parse_sc_title;

    #[test]
    fn meta_beats_uploader_when_title_has_no_artist() {
        // Реальный кейс: "benz" залит юзером "4", мета знает авторов.
        let c = ctx("benz", Some("4"), Some("ghasaii, psychosis"));
        let r = run_heuristic(&c);
        assert_eq!(names(&r), vec!["ghasaii", "psychosis"]);
        assert_eq!(r.source, ResolveSource::Meta);
        assert!((r.confidence - 0.5).abs() < 1e-6);
    }

    #[test]
    fn stylized_uploader_matches_meta_and_keeps_sc_link() {
        // "psychosis" от ᴍᴏɴᴀʀᴄʜ, мета "Monarch, johnertekker": оба артиста
        // в primary, аплоадер прилинкован к Monarch несмотря на смолкапсы.
        let c = ctx("psychosis", Some("ᴍᴏɴᴀʀᴄʜ"), Some("Monarch, johnertekker"));
        let r = run_heuristic(&c);
        assert_eq!(names(&r), vec!["Monarch", "johnertekker"]);
        assert_eq!(r.primary[0].sc_user_id.as_deref(), Some("42"));
        assert_eq!(r.primary[1].sc_user_id, None);
        assert!((r.confidence - 0.65).abs() < 1e-6);
    }

    #[test]
    fn reupload_with_label_meta_credits_real_artist() {
        // "Каждый день LIL KRYSTALLL" залит Sport1kk: мета должна победить
        // догадку «артист = загрузчик».
        let c = ctx(
            "Каждый день LIL KRYSTALLL",
            Some("Sport1kk"),
            Some("lil krystalll"),
        );
        let r = run_heuristic(&c);
        assert_eq!(names(&r), vec!["lil krystalll"]);
        assert_eq!(r.source, ResolveSource::Meta);
    }

    #[test]
    fn title_and_meta_union_coartists() {
        let c = ctx(
            "Dave Childz - Wish You Were Here",
            Some("JBroadway"),
            Some("Dave Childz, JBroadway"),
        );
        let r = run_heuristic(&c);
        assert_eq!(names(&r), vec!["Dave Childz", "JBroadway"]);
        assert_eq!(r.source, ResolveSource::Meta);
    }

    #[test]
    fn title_wins_when_meta_disjoint() {
        let c = ctx("Senso - minimal", Some("BakedEye"), Some("SUICIDAL AVENUE"));
        let r = run_heuristic(&c);
        assert_eq!(names(&r), vec!["Senso"]);
        assert_eq!(r.source, ResolveSource::Heuristic);
    }

    #[test]
    fn junk_meta_falls_back_to_uploader() {
        let c = ctx("ДВА КУСОЧКА ПИЦЦЫ", Some("GONE.Fludd"), Some("muzok.net"));
        let r = run_heuristic(&c);
        assert_eq!(names(&r), vec!["GONE.Fludd"]);
        assert_eq!(r.source, ResolveSource::Heuristic);
        assert!((r.confidence - 0.55).abs() < 1e-6);
    }

    #[test]
    fn meta_naming_only_producer_does_not_become_primary() {
        // "RAMPAGE (PROD. ZEMИSTERKAKTUS)" с метой "ZEMИSTERKAKTUS": мета
        // дублирует продюсера — primary остаётся за uploader'ом.
        let c = ctx(
            "RAMPAGE (PROD. ZEMИSTERKAKTUS)",
            Some("azulaqueen"),
            Some("ZEMИSTERKAKTUS"),
        );
        let r = run_heuristic(&c);
        assert_eq!(names(&r), vec!["azulaqueen"]);
        assert_eq!(r.source, ResolveSource::Heuristic);
        assert!(r
            .producers
            .iter()
            .any(|p| p.name.eq_ignore_ascii_case("ZEMИSTERKAKTUS")));
    }

    #[test]
    fn unspaced_chain_expands_via_meta() {
        // "ALUCIFYxBACKW666S - трек" + мета "alucify, backw666s".
        let c = ctx(
            "ALUCIFYxBACKW666S - sin city",
            Some("alucify"),
            Some("alucify, backw666s"),
        );
        let r = run_heuristic(&c);
        assert_eq!(names(&r), vec!["alucify", "backw666s"]);
        assert_eq!(r.source, ResolveSource::Meta);

        // Кириллический джойнер.
        let c2 = ctx("СОЛНЦЕхЛУНА - ночь", Some("кто-то"), Some("СОЛНЦЕ, ЛУНА"));
        let r2 = run_heuristic(&c2);
        assert_eq!(names(&r2), vec!["СОЛНЦЕ", "ЛУНА"]);
    }

    #[test]
    fn reversed_markup_unreversed_by_meta() {
        // "505 - arctic monkeys": мета знает правую часть, левая — номер/название.
        let c = ctx(
            "505 - arctic monkeys",
            Some("reposter"),
            Some("Arctic Monkeys"),
        );
        let s = LocalSignals::build_with_dictionary(&c, &HashSet::new());
        assert_eq!(s.parsed.primary_artists, vec!["arctic monkeys"]);
        assert_eq!(s.parsed.cleaned_title, "505");

        // Обычная разметка метой не переворачивается.
        let c2 = ctx("Psychosis - x-ray", None, Some("Psychosis"));
        let s2 = LocalSignals::build_with_dictionary(&c2, &HashSet::new());
        assert_eq!(s2.parsed.primary_artists, vec!["Psychosis"]);
        assert_eq!(s2.parsed.cleaned_title, "x-ray");
    }

    #[test]
    fn segmentation_splits_known_names_only() {
        // Оба сегмента в каталоге, склейка целиком — нет → режем.
        let mut parsed = parse_sc_title("Aikko Own Maslou - Место", None);
        let dict: HashSet<String> = ["aikko", "own maslou"]
            .into_iter()
            .map(String::from)
            .collect();
        segment_by_dictionary(&mut parsed, &dict);
        assert_eq!(parsed.primary_artists, vec!["Aikko", "Own Maslou"]);

        // Целое имя есть в каталоге → не трогаем ("Lil Peep").
        let mut whole = parse_sc_title("Lil Peep - Star Shopping", None);
        let dict2: HashSet<String> = ["lil peep", "lil", "peep"]
            .into_iter()
            .map(String::from)
            .collect();
        segment_by_dictionary(&mut whole, &dict2);
        assert_eq!(whole.primary_artists, vec!["Lil Peep"]);

        // Хвост не покрыт словарём → отказ, имя целиком.
        let mut partial = parse_sc_title("Sad Boy Loko - x", None);
        let dict3: HashSet<String> = ["sad boy"].into_iter().map(String::from).collect();
        segment_by_dictionary(&mut partial, &dict3);
        assert_eq!(partial.primary_artists, vec!["Sad Boy Loko"]);
    }

    #[test]
    fn segment_keys_cover_all_substrings() {
        let keys = segment_keys(&["Aikko", "Own", "Maslou"]);
        assert!(keys.contains(&"aikko".to_string()));
        assert!(keys.contains(&"own maslou".to_string()));
        assert!(keys.contains(&"aikko own maslou".to_string()));
        // Короче 3 символов — шум, в словарь не идёт.
        assert!(!segment_keys(&["Ян", "Ра"])
            .iter()
            .any(|k| k.chars().count() < 3));
    }
}
