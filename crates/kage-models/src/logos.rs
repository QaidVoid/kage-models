//! Provider logos, snapshotted from models.dev.
//!
//! kage's settings screens draw a provider's logo, and the GUI used to
//! reach models.dev for it at runtime. This module ends that: each logo
//! is fetched once at generation time, checked, and written under
//! `logos/<id>.svg`, which the workflow publishes beside the catalog.
//!
//! Two things make a reply unusable, and both are checked rather than
//! trusted:
//!
//! * models.dev answers an id it has no logo for with HTTP 200 and a
//!   generic sparkle glyph ([`PLACEHOLDER`]). It is a real SVG, so only
//!   comparing against it catches it.
//! * a body that is not an SVG at all reads as a failure here. A
//!   provider that starts serving an HTML error page would otherwise be
//!   snapshotted as a logo and drawn as a broken image.
//!
//! The id is a catalog key, so `commandcode` and `kenari` resolve from
//! their own entries; [`upstream_id`] maps the two kage ids whose logo
//! lives under another name.

use kage_provider::catalog::source::SUPPORTED_PROVIDERS;

/// The generic glyph models.dev serves, with HTTP 200, for an id it has
/// no logo for. A reply equal to this is a missing logo, not a logo.
///
/// Copied from `gui/src/logos.rs`, which used the same test. The two
/// copies must agree, so [`check`] rejects a reply that carries the
/// placeholder's distinguishing path data even when the surrounding
/// markup is reformatted upstream.
const PLACEHOLDER: &str = r#"<svg viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
  <path
    shape-rendering="geometricPrecision"
    d="M9.8132 15.9038L9 18.75L8.1868 15.9038C7.75968 14.4089 6.59112 13.2403 5.09619 12.8132L2.25 12L5.09619 11.1868C6.59113 10.7597 7.75968 9.59112 8.1868 8.09619L9 5.25L9.8132 8.09619C10.2403 9.59113 11.4089 10.7597 12.9038 11.1868L15.75 12L12.9038 12.8132C11.4089 13.2403 10.2403 14.4089 9.8132 15.9038Z"
    stroke="currentColor"
    stroke-width="1.5"
    stroke-linecap="round"
    stroke-linejoin="round"
  />
  <path
    d="M18.2589 8.71454L18 9.75L17.7411 8.71454C17.4388 7.50533 16.4947 6.56117 15.2855 6.25887L14.25 6L15.2855 5.74113C16.4947 5.43883 17.4388 4.49467 17.7411 3.28546L18 2.25L18.2589 3.28546C18.5612 4.49467 19.5053 5.43883 20.7145 5.74113L21.75 6L20.7145 6.25887C19.5053 6.56117 18.5612 7.50533 18.2589 8.71454Z"
    stroke="currentColor"
    stroke-width="1.5"
    stroke-linecap="round"
    stroke-linejoin="round"
  />
  <path
    d="M16.8942 20.5673L16.5 21.75L16.1058 20.5673C15.8818 19.8954 15.3546 19.3682 14.6827 19.1442L13.5 18.75L14.6827 18.3558C15.3546 18.1318 15.8818 17.6046 16.1058 16.9327L16.5 15.75L16.8942 16.9327C17.1182 17.6046 17.6454 18.1318 18.3173 18.3558L19.5 18.75L18.3173 19.1442C17.6454 19.3682 17.1182 19.8954 16.8942 20.5673Z"
    stroke="currentColor"
    stroke-width="1.5"
    stroke-linecap="round"
    stroke-linejoin="round"
  />
</svg>
"#;

/// Path data that appears only in [`PLACEHOLDER`]. Comparing the string
/// survives a reformat of the surrounding whitespace, which an exact
/// comparison would not.
const PLACEHOLDER_MARK: &str = "9.8132 15.9038";

/// A catalog key whose logo models.dev files under another name.
///
/// `openai` and `kimi-code-plan-cn` are catalog keys already, so the
/// kage ids that reuse them (`openai-responses`, `kimi-for-coding`)
/// need no entry of their own: the logo is published under the catalog
/// key and kage asks for that.
#[must_use]
pub fn upstream_id(catalog_key: &str) -> &str {
    match catalog_key {
        // models.dev lists neither provider's Claude variant, but
        // carries the parent provider's own mark. Without these two the
        // ids answer with [`PLACEHOLDER`] and kage would draw its
        // fallback glyph on a provider that does have a logo.
        "commandcode-claude" => "commandcode",
        "kenari-claude" => "kenari",
        other => other,
    }
}

/// The URL a catalog key's logo is read from.
///
/// # Panics
///
/// Never; the id is interpolated into a constant format.
#[must_use]
pub fn logo_url(catalog_key: &str, base: &str) -> String {
    format!(
        "{}/{}.svg",
        base.trim_end_matches('/'),
        upstream_id(catalog_key)
    )
}

/// Why a fetched body is not a logo, if it is not one.
///
/// Returns the bytes unchanged when they are usable. A body that is the
/// placeholder reads as [`LogoVerdict::Placeholder`], so the caller can
/// tell "no logo exists" from "the fetch was broken".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogoVerdict {
    /// Usable logo bytes.
    Ok,
    /// models.dev's generic glyph: this id has no logo.
    Placeholder,
    /// Not an SVG at all, so not something to draw.
    NotSvg,
}

/// Classify a fetched logo body.
///
/// An SVG is accepted on its opening tag alone, which is what the GUI's
/// loader needs; the placeholder is then ruled out both by exact
/// comparison and by its path data, so a reformat upstream does not turn
/// the placeholder into a logo. A body that opens with anything else, an
/// HTML error page included, is [`LogoVerdict::NotSvg`].
#[must_use]
pub fn check(body: &str) -> LogoVerdict {
    let trimmed = body.trim();
    if !trimmed.starts_with("<svg") && !trimmed.starts_with("<?xml") {
        return LogoVerdict::NotSvg;
    }
    if trimmed == PLACEHOLDER.trim() || body.contains(PLACEHOLDER_MARK) {
        return LogoVerdict::Placeholder;
    }
    LogoVerdict::Ok
}

/// Every catalog key kage needs a logo for, deduplicated and ordered.
#[must_use]
pub fn catalog_keys() -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = Vec::new();
    for map in SUPPORTED_PROVIDERS {
        if !keys.contains(&map.api_id) {
            keys.push(map.api_id);
        }
    }
    keys
}

/// Read every logo, keyed by catalog key.
///
/// A key whose fetch fails is absent rather than fatal: a logo is
/// decoration, and one provider's icon being unreachable should not
/// stop the catalog from being generated. The failures come back in
/// `warnings` so a provider that silently loses its logo is visible in
/// the run's output.
#[must_use]
pub fn fetch_all(base: &str) -> (std::collections::BTreeMap<String, String>, Vec<String>) {
    let mut logos = std::collections::BTreeMap::new();
    let mut warnings = Vec::new();
    for key in catalog_keys() {
        let url = logo_url(key, base);
        match crate::fetch::fetch(&url) {
            Ok(body) => match check(&body) {
                LogoVerdict::Ok => {
                    logos.insert(key.to_owned(), normalize(&body));
                }
                LogoVerdict::Placeholder => {
                    warnings.push(format!("{key}: models.dev has no logo for it"));
                }
                LogoVerdict::NotSvg => {
                    warnings.push(format!("{key}: {url} is not an SVG"));
                }
            },
            Err(e) => warnings.push(format!("{key}: {e}")),
        }
    }
    (logos, warnings)
}

/// The bytes as they will be written, so a re-fetch that differs only in
/// trailing whitespace does not churn the tree.
///
/// Models.dev's SVGs are hand-written and inconsistent about the final
/// newline; normalizing here means the committed file matches what the
/// check expects.
fn normalize(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str = r##"<svg viewBox="0 0 32 32" xmlns="http://www.w3.org/2000/svg"><path d="M1 1h4v4H1z" fill="#000"/></svg>"##;

    #[test]
    fn a_real_logo_is_accepted() {
        assert_eq!(check(REAL), LogoVerdict::Ok);
    }

    #[test]
    fn the_placeholder_is_not_a_logo() {
        assert_eq!(check(PLACEHOLDER), LogoVerdict::Placeholder);
    }

    #[test]
    fn html_is_not_a_logo() {
        // A provider whose host answers /favicon.ico with a parked
        // page serves this for its logo.
        assert_eq!(
            check("<!DOCTYPE html><html><head><title>x</title>"),
            LogoVerdict::NotSvg
        );
        assert_eq!(
            check("<html lang=\"en\"><body>404</body></html>"),
            LogoVerdict::NotSvg
        );
    }

    #[test]
    fn an_empty_body_is_not_a_logo() {
        assert_eq!(check(""), LogoVerdict::NotSvg);
        assert_eq!(check("   \n"), LogoVerdict::NotSvg);
    }

    #[test]
    fn an_xml_declared_svg_is_accepted() {
        let body = format!("<?xml version=\"1.0\"?>{REAL}");
        assert_eq!(check(&body), LogoVerdict::Ok);
    }

    #[test]
    fn the_placeholder_is_caught_through_a_reformat() {
        // The exact text differs from the pinned constant, but the path
        // data is the placeholder's alone.
        let reformatted = PLACEHOLDER.replace('\n', " ").replace("  ", " ");
        assert_ne!(reformatted, PLACEHOLDER);
        assert_eq!(check(&reformatted), LogoVerdict::Placeholder);
    }

    #[test]
    fn the_claude_variants_read_their_parent_providers_logo() {
        assert_eq!(upstream_id("commandcode-claude"), "commandcode");
        assert_eq!(upstream_id("kenari-claude"), "kenari");
        assert_eq!(upstream_id("commandcode"), "commandcode");
        assert_eq!(upstream_id("kenari"), "kenari");
        assert_eq!(upstream_id("groq"), "groq");
    }

    #[test]
    fn logo_urls_join_the_base_without_doubling_a_slash() {
        assert_eq!(
            logo_url("groq", "https://models.dev/logos"),
            "https://models.dev/logos/groq.svg"
        );
        assert_eq!(
            logo_url("commandcode-claude", "https://models.dev/logos/"),
            "https://models.dev/logos/commandcode.svg"
        );
    }

    #[test]
    fn every_catalog_key_is_covered_once() {
        let keys = catalog_keys();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(keys.len(), sorted.len(), "a key is listed twice: {keys:?}");
        assert!(keys.contains(&"commandcode-claude"));
        assert!(keys.contains(&"openai"));
        assert!(keys.contains(&"kenari"));
    }

    #[test]
    fn normalizing_settles_the_trailing_newline() {
        assert_eq!(normalize("  <svg/>  \n\n"), "<svg/>\n");
        assert_eq!(normalize("<svg/>"), "<svg/>\n");
        assert_eq!(normalize("   "), "");
    }
}
