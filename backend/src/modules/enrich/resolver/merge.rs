//! Слияние внешнего результата с локальными сигналами: внешние источники
//! часто знают только первого исполнителя, полный состав — разметка заголовка
//! ("Psychosis, LEYNCLOUD, inxwertg - …") и лейбловая мета.

use crate::modules::enrich::artist_names;

use super::signals::{name_matches_uploader, LocalSignals};
use super::{ArtistCandidate, ResolveResult, TrackContext};

/// Внешний результат + локальные сигналы. Роли, которых внешний источник не
/// знает, берутся из эвристики; co-primary добираются из разметки и меты.
pub(super) fn merge_with(
    heuristic: ResolveResult,
    ext_res: ResolveResult,
    ctx: &TrackContext,
    signals: &LocalSignals,
) -> ResolveResult {
    let mut out = ext_res;
    if out.primary.is_empty() {
        out.primary = heuristic.primary.clone();
    } else {
        attach_uploader_id(&mut out.primary, ctx);
    }
    if out.producers.is_empty() {
        out.producers = heuristic.producers;
    }
    if out.remixers.is_empty() {
        out.remixers = heuristic.remixers;
    }
    if out.featured.is_empty() {
        out.featured = heuristic.featured;
    }

    if let Some(markup) = signals.markup() {
        extend_missing_coprimary(&mut out, markup, ctx);
    }
    extend_missing_coprimary(&mut out, &signals.meta_names, ctx);
    out
}

/// Обогатить fast-path результат (sc_verified) локальными сигналами так же,
/// как внешние: featured/prod/remix из парса, co-primary из разметки и меты.
/// Без этого verified-клейм затирал перечисленных в заголовке соавторов
/// ("мокери, psychosis - …" → только МОКЕРИ) и мету ("takizava & dekma").
pub fn enrich_with_local_signals(
    fast: ResolveResult,
    ctx: &TrackContext,
    signals: &LocalSignals,
) -> ResolveResult {
    merge_with(signals.heuristic(ctx), fast, ctx, signals)
}

/// Кандидату с именем uploader'а — его sc_user_id (любой позиции в составе,
/// не только первой: uploader бывает вторым в "A, uploader - трек").
fn attach_uploader_id(primary: &mut [ArtistCandidate], ctx: &TrackContext) {
    let Some(sc_id) = ctx.uploader_sc_user_id.as_deref() else {
        return;
    };
    for cand in primary.iter_mut() {
        if cand.sc_user_id.is_none()
            && name_matches_uploader(&cand.name, ctx.uploader_username.as_deref())
        {
            cand.sc_user_id = Some(sc_id.to_string());
        }
    }
}

/// Добрать в primary имена из `names` (разметка/мета), которых ещё нет ни в
/// одной роли. Срабатывает только когда `names` пересекается с уже найденным
/// составом — это защита от чужих/мусорных списков.
fn extend_missing_coprimary(out: &mut ResolveResult, names: &[String], ctx: &TrackContext) {
    if out.primary.is_empty() || names.len() < 2 {
        return;
    }
    let primary_names: Vec<String> = out.primary.iter().map(|c| c.name.clone()).collect();
    let agrees = names
        .iter()
        .any(|m| artist_names::name_in(m, primary_names.iter().map(|s| s.as_str())));
    if !agrees {
        return;
    }
    let known: Vec<String> = out
        .primary
        .iter()
        .chain(out.featured.iter())
        .chain(out.producers.iter())
        .chain(out.remixers.iter())
        .map(|c| c.name.clone())
        .collect();
    for m in names {
        if !artist_names::name_in(m, known.iter().map(|s| s.as_str())) {
            let sc_user_id = if name_matches_uploader(m, ctx.uploader_username.as_deref()) {
                ctx.uploader_sc_user_id.clone()
            } else {
                None
            };
            out.primary.push(ArtistCandidate {
                name: m.clone(),
                mb_id: None,
                genius_id: None,
                sc_user_id,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{ctx, names, run_heuristic, signals_no_dict};
    use super::*;
    use crate::modules::enrich::resolver::{ResolveSource, TrackContext};

    fn merge_for(c: &TrackContext, ext: ResolveResult) -> ResolveResult {
        let signals = signals_no_dict(c);
        merge_with(run_heuristic(c), ext, c, &signals)
    }

    #[test]
    fn merge_appends_missing_meta_coprimary() {
        // Genius нашёл только Psychosis, мета знает второго.
        let c = ctx("паралич", Some("Psychosis"), Some("Psychosis, killaheelz"));
        let ext = ResolveResult {
            source: ResolveSource::Genius,
            confidence: 0.8,
            primary: vec![ArtistCandidate {
                name: "Psychosis".into(),
                mb_id: None,
                genius_id: Some("123".into()),
                sc_user_id: None,
            }],
            ..Default::default()
        };
        let merged = merge_for(&c, ext);
        assert_eq!(names(&merged), vec!["Psychosis", "killaheelz"]);
        assert_eq!(merged.source, ResolveSource::Genius);
    }

    #[test]
    fn merge_skips_meta_when_disjoint_from_external() {
        let c = ctx("song", Some("up"), Some("Akio Ohmori, Ritsuo Kamimura"));
        let ext = ResolveResult {
            source: ResolveSource::Genius,
            confidence: 0.7,
            primary: vec![ArtistCandidate {
                name: "Cyalm".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let merged = merge_for(&c, ext);
        assert_eq!(names(&merged), vec!["Cyalm"]);
    }

    #[test]
    fn markup_coartists_added_to_external_result() {
        // Genius вернул только Psychosis, разметка перечисляет троих.
        let c = ctx(
            "Psychosis, LEYNCLOUD, inxwertg - blade mail",
            Some("0n3PunchMan"),
            None,
        );
        let ext = ResolveResult {
            source: ResolveSource::Genius,
            confidence: 0.8,
            primary: vec![ArtistCandidate {
                name: "Psychosis".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let merged = merge_for(&c, ext);
        assert_eq!(names(&merged), vec!["Psychosis", "LEYNCLOUD", "inxwertg"]);
    }

    #[test]
    fn fast_path_enriched_with_markup_and_meta() {
        // sc_verified клейм МОКЕРИ + разметка "мокери, psychosis" → оба.
        let c = ctx("мокери, psychosis - no.happiness", Some("МОКЕРИ"), None);
        let fast = ResolveResult {
            source: ResolveSource::ScVerified,
            confidence: 1.0,
            primary: vec![ArtistCandidate {
                name: "МОКЕРИ".into(),
                sc_user_id: Some("42".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let signals = signals_no_dict(&c);
        let out = enrich_with_local_signals(fast, &c, &signals);
        assert_eq!(names(&out), vec!["МОКЕРИ", "psychosis"]);
        assert_eq!(out.source, ResolveSource::ScVerified);

        // dekma-класс: разметки нет, мета знает второго.
        let c2 = ctx("без шансов", Some("dekma"), Some("takizava & dekma"));
        let fast2 = ResolveResult {
            source: ResolveSource::ScVerified,
            confidence: 1.0,
            primary: vec![ArtistCandidate {
                name: "dekma".into(),
                sc_user_id: Some("42".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let signals2 = signals_no_dict(&c2);
        let out2 = enrich_with_local_signals(fast2, &c2, &signals2);
        assert_eq!(names(&out2), vec!["dekma", "takizava"]);
    }

    #[test]
    fn fast_path_gets_unreversed_markup() {
        // M1-фикс: verified-путь видит тот же unreverse, что и каскад.
        // "505 - arctic monkeys" + мета: локальные сигналы знают что артист
        // справа, fast-path не плодит co-primary "505".
        let c = ctx(
            "505 - arctic monkeys",
            Some("arcticmonkeys"),
            Some("Arctic Monkeys"),
        );
        let fast = ResolveResult {
            source: ResolveSource::ScVerified,
            confidence: 1.0,
            primary: vec![ArtistCandidate {
                name: "Arctic Monkeys".into(),
                sc_user_id: Some("42".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let signals = signals_no_dict(&c);
        let out = enrich_with_local_signals(fast, &c, &signals);
        assert_eq!(names(&out), vec!["Arctic Monkeys"]);
    }

    #[test]
    fn merge_does_not_duplicate_featured_from_meta() {
        // Мета перечисляет и фитующих — они уже в featured, в primary не дублируем.
        let c = ctx(
            "GLAM GO! - ГЛЯНЬ ЕЙ НА ЛИЦО (feat. Gone.Fludd)",
            Some("glamgo"),
            Some("Glam Go, Gone.Fludd"),
        );
        let ext = ResolveResult {
            source: ResolveSource::Genius,
            confidence: 0.8,
            primary: vec![ArtistCandidate {
                name: "GLAM GO GANG!".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let merged = merge_for(&c, ext);
        assert_eq!(names(&merged), vec!["GLAM GO GANG!"]);
        assert!(merged
            .featured
            .iter()
            .any(|f| f.name.to_lowercase().contains("fludd")));
    }

    #[test]
    fn uploader_id_attached_to_any_position() {
        // Uploader второй в составе — sc_user_id всё равно прикрепляется.
        let c = ctx("A, uploader - song", Some("uploader"), None);
        let ext = ResolveResult {
            source: ResolveSource::Genius,
            confidence: 0.8,
            primary: vec![
                ArtistCandidate {
                    name: "A".into(),
                    ..Default::default()
                },
                ArtistCandidate {
                    name: "uploader".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let merged = merge_for(&c, ext);
        assert_eq!(merged.primary[1].sc_user_id.as_deref(), Some("42"));
    }
}
