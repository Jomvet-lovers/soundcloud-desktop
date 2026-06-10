//! Fast-path: трек на аккаунте, привязанном к артисту каталога (main,
//! verified) — клеймим без внешних API, если локальные сигналы не возражают.

use tracing::debug;

use crate::error::AppResult;
use crate::modules::enrich::artist_names;

use super::signals::LocalSignals;
use super::{ArtistCandidate, ResolveResult, ResolveSource, TrackContext};

/// Verified-клейм владельца аккаунта. None — аккаунт не привязан или гейты
/// отдали трек полному каскаду.
pub(super) async fn claim(
    ctx: &TrackContext,
    signals: &LocalSignals,
    pg: &sqlx::PgPool,
) -> AppResult<Option<ResolveResult>> {
    let Some(uploader_sc_id) = ctx.uploader_sc_user_id.as_deref() else {
        return Ok(None);
    };
    if uploader_sc_id.is_empty() {
        return Ok(None);
    }
    let row = sqlx::query_file!(
        "queries/enrich/service/sc_verified_artist.sql",
        uploader_sc_id
    )
    .fetch_optional(pg)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };

    if let Err(reason) = claim_gate(&row.name, signals) {
        debug!(uploader_sc_id, mapped = %row.name, reason, "sc_verified skipped");
        return Ok(None);
    }

    Ok(Some(ResolveResult {
        source: ResolveSource::ScVerified,
        confidence: 1.0,
        primary: vec![ArtistCandidate {
            name: row.name,
            mb_id: row.mb_artist_id,
            genius_id: row.genius_artist_id,
            sc_user_id: Some(uploader_sc_id.to_string()),
        }],
        isrc: ctx.isrc.clone(),
        ..Default::default()
    }))
}

/// Гейты клейма: разметка заголовка называет другого артиста или живая мета
/// не содержит владельца аккаунта → трек чужой (реаплоад/гость на канале),
/// пусть решает полный каскад.
///
/// Сравнение — `same_artist` (единая шкала): стилизация/ё/регистр не должны
/// выключать fast-path. Uploader-fallback разметкой НЕ считается — иначе
/// артист с аккаунтом, чей username не совпадает с именем (Drake /
/// champagnepapi), терял бы fast-path на каждом треке без дефиса.
fn claim_gate(artist_name: &str, signals: &LocalSignals) -> Result<(), &'static str> {
    if signals.parsed.primary_from_title {
        let title_claims_other = signals
            .parsed
            .primary_artists
            .first()
            .map(|p| !artist_names::same_artist(p, artist_name))
            .unwrap_or(false);
        if title_claims_other {
            return Err("title claims different artist");
        }
    }
    // Живая мета, в которой артиста нет — трек чужой ("DISTORTED DREAMS"
    // c метой "frxchtzwxrg & m∞nflower" на аккаунте Zemix).
    if !signals.meta_names.is_empty()
        && !artist_names::name_in(artist_name, signals.meta_names.iter().map(|s| s.as_str()))
    {
        return Err("metadata names other artists");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{ctx, signals_no_dict};
    use super::*;

    fn gate(
        artist: &str,
        title: &str,
        uploader: Option<&str>,
        meta: Option<&str>,
    ) -> Result<(), &'static str> {
        let c = ctx(title, uploader, meta);
        let signals = signals_no_dict(&c);
        claim_gate(artist, &signals)
    }

    #[test]
    fn gate_passes_own_and_styled_markup() {
        // Своя разметка — пропускаем, включая стилизацию и со-авторов.
        assert!(gate("МОКЕРИ", "МОКЕРИ - kill", Some("МОКЕРИ"), None).is_ok());
        assert!(gate(
            "МОКЕРИ",
            "мокери, psychosis - no.happiness",
            Some("МОКЕРИ"),
            None
        )
        .is_ok());
        assert!(gate("Monarch", "ᴍᴏɴᴀʀᴄʜ - psychosis", Some("ᴍᴏɴᴀʀᴄʜ"), None).is_ok());
    }

    #[test]
    fn gate_blocks_foreign_markup() {
        // Разметка называет другого — не клеймим.
        assert_eq!(
            gate("Zemix", "Drake - God's Plan", Some("Zemix"), None),
            Err("title claims different artist")
        );
    }

    #[test]
    fn gate_ignores_uploader_fallback() {
        // Без разметки primary = uploader-fallback; username аккаунта может
        // не совпадать с именем артиста — это НЕ "title claims other".
        assert!(gate("Drake", "God's Plan", Some("champagnepapi"), None).is_ok());
    }

    #[test]
    fn gate_blocks_foreign_meta() {
        // Мета называет других — трек чужой (DISTORTED DREAMS / Zemix).
        assert_eq!(
            gate(
                "Zemix",
                "DISTORTED DREAMS (ft. m∞nflower)",
                Some("Zemix"),
                Some("frxchtzwxrg & m∞nflower")
            ),
            Err("metadata names other artists")
        );
        // Мета содержит артиста — пропускаем.
        assert!(gate(
            "dekma",
            "без шансов",
            Some("dekma"),
            Some("takizava & dekma")
        )
        .is_ok());
    }
}
