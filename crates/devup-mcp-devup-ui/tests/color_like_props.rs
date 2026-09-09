//! `hardcoded-color` and the props it is allowed to look at.
//!
//! The rule only runs on props listed as color-like, so a prop missing
//! from that list is not a weaker check - it is no check. `bg` was
//! missing, and `bg` is what the Figma generator writes for every solid
//! fill, so the most common hardcoded color this repository produces was
//! the one nothing examined. Reading the output back, that silence looked
//! like a deliberate rule ("colors are only reported when a matching token
//! exists") and was written up as one. These lock the rule to the props.

use devup_mcp_devup_ui::theme::{ProjectTheme, parse_project_theme};
use devup_mcp_devup_ui::ui_validate::{Severity, validate_devup_ui_tsx};

fn theme() -> ProjectTheme {
    parse_project_theme(
        r##"{ "theme": {
            "colors": { "default": { "primary": "#752D2D", "surface": "#FFFFFF" } },
            "typography": {},
            "length": { "default": { "md": "16px" } },
            "shadow": {}
        } }"##,
    )
    .expect("fixture devup.json parses")
}

fn colors_flagged(tsx: &str) -> Vec<String> {
    validate_devup_ui_tsx(tsx, Some(&theme()), false)
        .violations
        .into_iter()
        .filter(|violation| violation.rule == "hardcoded-color")
        .map(|violation| violation.message)
        .collect()
}

/// The exact shape `devup_figma_export` emits for a solid fill.
#[test]
fn a_hardcoded_color_on_bg_is_reported() {
    let flagged = colors_flagged(r##"export const S = () => <Box bg="#752E2E" />;"##);
    assert_eq!(flagged.len(), 1, "{flagged:?}");
    assert!(flagged[0].contains("bg"), "{flagged:?}");
    assert!(flagged[0].contains("#752E2E"), "{flagged:?}");
}

/// Writing the same color the long way was always reported. The shorthand
/// has to agree, or the rule depends on which spelling the author chose.
#[test]
fn the_shorthand_and_the_long_form_agree() {
    let shorthand = colors_flagged(r##"export const S = () => <Box bg="#752E2E" />;"##);
    let long_form = colors_flagged(r##"export const S = () => <Box bgColor="#752E2E" />;"##);
    assert_eq!(shorthand.len(), long_form.len());
    assert_eq!(shorthand.len(), 1);
}

/// Every color literal is reported. Exact matches are actionable warnings;
/// unmatched values are info with no speculative token suggestion (INT1/D11).
#[test]
fn colors_are_always_reported_but_only_exact_matches_warn_and_suggest() {
    let suggestion_for = |value: &str| {
        validate_devup_ui_tsx(
            &format!(r##"export const S = () => <Box bg="{value}" />;"##),
            Some(&theme()),
            false,
        )
        .violations
        .into_iter()
        .find(|violation| violation.rule == "hardcoded-color")
        .map(|violation| (violation.severity, violation.suggestion))
    };

    // Exactly `$primary`'s value: reported, with the token named.
    let exact = suggestion_for("#752D2D").expect("an exact-value color is reported");
    assert!(
        exact.1.is_some_and(|text| text.contains("$primary")),
        "the token whose value this is should be suggested"
    );

    assert_eq!(exact.0, Severity::Warning);
    // One digit away, and nothing else close: still reported, nothing to
    // suggest. Suggestions are exact-value only.
    let near = suggestion_for("#752E2E").expect("a near-miss color is still reported");
    assert!(
        near.1.is_none(),
        "no token holds this value, so none is named"
    );

    assert_eq!(serde_json::to_value(near.0).unwrap(), "info");
    // Nothing like it in the theme at all: still reported.
    let far = suggestion_for("#0A9F4C").expect("an unrelated color is still reported");
    assert!(far.1.is_none());
    assert_eq!(serde_json::to_value(far.0).unwrap(), "info");
}

/// `bg` also takes images and gradients. Only a color literal is a color,
/// and the detector requires a leading `#`, so these must stay silent.
#[test]
fn a_non_color_background_value_is_not_reported() {
    for tsx in [
        r##"export const S = () => <Box bg="url('/images/hero.png')" />;"##,
        r##"export const S = () => <Box bg="linear-gradient(180deg, red, blue)" />;"##,
        r##"export const S = () => <Box bg="transparent" />;"##,
        r##"export const S = () => <Box bg="$primary" />;"##,
    ] {
        assert!(
            colors_flagged(tsx).is_empty(),
            "should not be read as a hardcoded color: {tsx}"
        );
    }
}

/// Having "color" in the name does not make a prop color-valued.
#[test]
fn a_keyword_valued_prop_named_color_is_not_reported() {
    assert!(colors_flagged(r##"export const S = () => <Box colorScheme="dark" />;"##).is_empty());
}
