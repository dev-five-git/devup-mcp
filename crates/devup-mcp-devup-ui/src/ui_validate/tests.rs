

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::parse_project_theme;

    #[test]
    fn effective_navigation_attributes_and_transitions_are_recognized() {
        for attrs in [
            r#"props={{ href: '/x', 'aria-current': active ? 'page' : undefined }} transition="color .2s""#,
            r#"{...{ props: { href: '/x', ['aria-current']: active ? 'page' : undefined }, _hover: { transition: 'color .2s' } }}"#,
            r#"props={active ? { href: '/x', 'aria-current': 'page' } : { href: '/x', 'aria-current': undefined }} selectors={{ '&:hover': { transitionDuration: '200ms' } }}"#,
            r#"props={{ href: '/x', 'aria-current': active ? 'page' : undefined }} className={css({ transition: 'color .2s' })}"#,
            r#"props={{ href: '/x', 'aria-current': active ? 'page' : undefined }} className={linkStyle}"#,
        ] {
            let source = format!(r#"import RouteLink from 'next/link'; const linkStyle = css({{ transition: 'color .2s' }}); const Header = () => <nav><Text as={{RouteLink}} {attrs} _hover={{{{color: '$primary'}}}} _focusVisible={{{{outline: 'solid'}}}}>Company</Text></nav>"#);
            let findings = site::interaction_findings("Header.tsx", &source);
            assert!(!findings.iter().any(|v| matches!(v.rule, "nav-current" | "hover-transition")), "{source}: {findings:?}");
            assert_eq!(site::interaction_coverage("Header.tsx", &source).interactive, 1);
        }
    }

    #[test]
    fn narrow_safe_rows_do_not_report_fixed_columns() {
        for source in [
            r#"<Flex display={['none',null,null,null,'flex']}><Box w="400px"/><Box w="400px"/></Flex>"#,
            r#"<Flex><Box display={['none', 'block']} w="400px"/><Box display={['none','block']} w="400px"/></Flex>"#,
            r#"<Flex flexDir={['column', 'row']}><Box w="400px"/><Box w="400px"/></Flex>"#,
            r#"<Flex w={['100%',null,'1200px']}><Box w={['100%',null,'400px']}/><Box w={['auto',null,'400px']}/></Flex>"#,
        ] {
            let report = validate_devup_ui_tsx(source, None, false);
            assert!(!report.violations.iter().any(|v| v.rule == "narrow-fixed-columns"), "{source}: {:?}", report.violations);
        }
    }

    #[test]
    fn prop_sort_findings_are_one_lint_fix_action() {
        let report = serde_json::to_value(validate_devup_ui_tsx(
            r#"<><Box p={0} boxSize="8px" /><Box props={{}} pr="16px" /></>"#,
            None,
            false,
        )).unwrap();
        assert_eq!(report["nextAction"]["command"], "bun run lint:fix");
        assert_eq!(report["nextAction"]["fixable"], 2);
        assert_eq!(report["nextAction"]["label"], "bun run lint:fix (fixable 2)");
    }

    #[test]
    fn prop_sort_respects_reserved_props_and_spread_boundaries() {
        for tsx in [
            r#"<Box key="a" ref={ref} appearance="none" aria-expanded={false} />"#,
            r#"<Box zIndex={1} {...props} appearance="none" />"#,
            r#"<Box appearance="none" boxSize="8px" p={0} />"#,
        ] {
            let report = serde_json::to_value(validate_devup_ui_tsx(tsx, None, false)).unwrap();
            assert!(report["nextAction"].is_null(), "{tsx}: {report}");
        }
    }

    fn fixture_theme() -> ProjectTheme {
        parse_project_theme(
            r##"{ "theme": {
                "colors": { "default": { "captionLight": "#999999", "backgroundLight": "#fafafa" } },
                "typography": {},
                "length": { "default": { "sm": "8px", "md": "16px" } },
                "shadow": {}
            } }"##,
        )
        .unwrap()
    }

    #[test]
    fn catches_the_gray100_regression_case() {
        let tsx = r##"export const Bubble = () => <Box bg="$gray100" />;"##;
        let report = validate_devup_ui_tsx(tsx, Some(&fixture_theme()), false);
        assert!(!report.ok);
        assert!(
            report
                .violations
                .iter()
                .any(|violation| violation.rule == "unknown-token"
                    && violation.message.contains("gray100"))
        );
    }

    #[test]
    fn allows_existing_tokens() {
        let tsx = r##"export const Bubble = () => <Box bg="$captionLight" />;"##;
        let report = validate_devup_ui_tsx(tsx, Some(&fixture_theme()), false);
        assert!(report.ok, "{:?}", report.violations);
        assert_eq!(report.checked_tokens, 1);
    }

    /// A real project's `devup.json` (`braillify-studio` `apps/front`): colors
    /// and typography side by side, where the color the generator wanted is
    /// one edit away under a different name.
    fn theme_with_colors_and_typography() -> ProjectTheme {
        parse_project_theme(
            r##"{ "theme": {
                "colors": { "light": {
                    "bg": "#FFFFFF", "border": "#E5E5E5",
                    "text": "#111111", "textSubtle": "#9A9A9A"
                } },
                "typography": {
                    "bodyStrong": { "fontSize": "14px" },
                    "button": { "fontSize": "15px" },
                    "caption": { "fontSize": "12px" }
                },
                "length": {}, "shadow": {}
            } }"##,
        )
        .unwrap()
    }

    /// A token named in a color prop can only be replaced by a color token.
    /// Ranking the whole catalog by edit distance answered `bg="$background"`
    /// with `$bodyStrong, $button, $caption` — three typography tokens, none
    /// of which is usable here — while `$bg`, the one the project actually
    /// defines for this, did not appear at all.
    #[test]
    fn unknown_color_token_is_never_advised_to_use_a_typography_token() {
        let tsx = r##"export const X = () => <Box bg="$background" />;"##;
        let report = validate_devup_ui_tsx(tsx, Some(&theme_with_colors_and_typography()), false);
        let violation = report
            .violations
            .iter()
            .find(|violation| violation.rule == "unknown-token")
            .expect("unknown-token violation");
        let suggestion = violation
            .suggestion
            .as_deref()
            .expect("a theme with color tokens can always advise one");
        for typography in ["$bodyStrong", "$button", "$caption"] {
            assert!(
                !suggestion.contains(typography),
                "{typography} cannot be used in a color prop: {suggestion}"
            );
        }
        assert!(
            suggestion.contains("$bg"),
            "the color token this project defines must be offered: {suggestion}"
        );
    }

    /// The same rule in the other direction: a length prop is advised with
    /// length tokens, never with the colors that happen to be spelled alike.
    #[test]
    fn unknown_length_token_is_advised_only_with_length_tokens() {
        let theme = parse_project_theme(
            r##"{ "theme": {
                "colors": { "default": { "gutterColor": "#FFFFFF" } },
                "typography": {},
                "length": { "default": { "gutter": "16px" } },
                "shadow": {}
            } }"##,
        )
        .unwrap();
        let tsx = r##"export const X = () => <Box p="$gutterX" />;"##;
        let report = validate_devup_ui_tsx(tsx, Some(&theme), false);
        let violation = report
            .violations
            .iter()
            .find(|violation| violation.rule == "unknown-token")
            .expect("unknown-token violation");
        let suggestion = violation.suggestion.as_deref().expect("length advice");
        assert!(
            !suggestion.contains("$gutterColor"),
            "a color token cannot be used in a length prop: {suggestion}"
        );
        assert!(
            suggestion.contains("$gutter"),
            "the length token this project defines must be offered: {suggestion}"
        );
    }

    /// A theme that names its typography, as every real one does.
    fn theme_with_typography() -> ProjectTheme {
        parse_project_theme(
            r##"{ "theme": {
                "colors": { "default": { "text": "#111111" } },
                "typography": {
                    "body": { "fontSize": "14px" },
                    "bodyL": { "fontSize": "16px" },
                    "h1": { "fontSize": "36px" }
                },
                "length": {}, "shadow": {}
            } }"##,
        )
        .unwrap()
    }

    /// devup-ui's `typography` prop names a theme token without a `$`, so the
    /// `$`-prefixed check never saw one. A generated Braillify Studio screen
    /// carried `typography="mainSubText"`, `"titleSmMed"` and `"bodyMed"` —
    /// none of them in that project's devup.json — and validation reported
    /// six unknown tokens, every one of them a color, and said nothing about
    /// these three. An unknown typography token is a style that silently does
    /// not apply.
    #[test]
    fn an_unknown_typography_token_is_reported() {
        let tsx = r##"export const X = () => <Text typography="mainSubText" />;"##;
        let report = validate_devup_ui_tsx(tsx, Some(&theme_with_typography()), false);
        let violation = report
            .violations
            .iter()
            .find(|violation| violation.rule == "unknown-token")
            .unwrap_or_else(|| panic!("mainSubText is not in the theme: {:?}", report.violations));
        assert_eq!(violation.severity, Severity::Error);
        assert!(violation.message.contains("mainSubText"), "{violation:?}");
        assert!(!report.ok);
    }

    /// The advice for one names the others, and only the others.
    #[test]
    fn an_unknown_typography_token_is_advised_with_typography_tokens() {
        let tsx = r##"export const X = () => <Text typography="bodyMed" />;"##;
        let report = validate_devup_ui_tsx(tsx, Some(&theme_with_typography()), false);
        let violation = report
            .violations
            .iter()
            .find(|violation| violation.rule == "unknown-token")
            .expect("bodyMed is not in the theme");
        let suggestion = violation.suggestion.as_deref().expect("typography advice");
        assert!(suggestion.contains("body"), "{suggestion}");
        assert!(
            !suggestion.contains("$text"),
            "a color cannot be a typography token: {suggestion}"
        );
    }

    /// A token the theme does define is not flagged, and is counted as checked.
    #[test]
    fn a_defined_typography_token_passes() {
        let tsx = r##"export const X = () => <Text typography="bodyL" />;"##;
        let report = validate_devup_ui_tsx(tsx, Some(&theme_with_typography()), false);
        assert!(report.ok, "{:?}", report.violations);
        assert_eq!(report.checked_tokens, 1);
    }

    /// With no typography in the theme there is nothing to check against, and
    /// a check that cannot be made is skipped rather than guessed — the rule
    /// `a_theme_with_no_length_tokens_is_never_told_to_use_one` already holds
    /// for lengths.
    #[test]
    fn a_theme_with_no_typography_tokens_flags_nothing() {
        let tsx = r##"export const X = () => <Text typography="body" />;"##;
        let report = validate_devup_ui_tsx(tsx, Some(&fixture_theme()), false);
        assert!(report.ok, "{:?}", report.violations);
        assert!(
            report
                .violations
                .iter()
                .all(|violation| violation.rule != "unknown-token"),
            "{:?}",
            report.violations
        );
    }

    #[test]
    fn flags_hardcoded_hex_color_with_suggestion() {
        let tsx = r##"export const X = () => <Box color="#999999" />;"##;
        let report = validate_devup_ui_tsx(tsx, Some(&fixture_theme()), false);
        let violation = report
            .violations
            .iter()
            .find(|violation| violation.rule == "hardcoded-color")
            .expect("hardcoded-color violation");
        assert_eq!(violation.severity, Severity::Warning);
        assert!(
            violation
                .suggestion
                .as_deref()
                .unwrap()
                .contains("captionLight")
        );
    }

    #[test]
    fn flags_hardcoded_px_length_with_suggestion() {
        let tsx = r##"export const X = () => <Box borderRadius="16px" />;"##;
        let report = validate_devup_ui_tsx(tsx, Some(&fixture_theme()), false);
        let violation = report
            .violations
            .iter()
            .find(|violation| violation.rule == "hardcoded-length")
            .expect("hardcoded-length violation");
        assert!(violation.suggestion.as_deref().unwrap().contains("md"));
    }

    /// A real project's `devup.json` (`devup-mcp-defects.md` D11): colors
    /// and typography, and no `length` key at all.
    fn theme_without_length_tokens() -> ProjectTheme {
        parse_project_theme(
            r##"{ "theme": {
                "colors": { "default": { "primary": "#752D2D" } },
                "typography": { "h4": { "fontSize": "20px" } }
            } }"##,
        )
        .unwrap()
    }

    #[test]
    fn a_theme_with_no_length_tokens_is_never_told_to_use_one() {
        // Every one of these is a length the generator emitted against a
        // theme that defines zero length tokens. Advising a token when the
        // project has none to use is advice that cannot be followed.
        let tsx = r##"export const X = () => (
            <Box gap="40px" py="80px" h="34px" w="64px" px="40px" p="20px"
                 borderRadius="2000px" boxSize="48px" />
        );"##;
        let report = validate_devup_ui_tsx(tsx, Some(&theme_without_length_tokens()), false);
        let lengths = report
            .violations
            .iter()
            .filter(|violation| violation.rule == "hardcoded-length")
            .collect::<Vec<_>>();
        assert_eq!(
            lengths.len(),
            8,
            "every literal remains visible: {lengths:?}"
        );
        for finding in lengths {
            assert_eq!(serde_json::to_value(finding).unwrap()["severity"], "info");
            assert!(finding.suggestion.is_none());
            assert!(finding.message.contains("no matching token"));
        }
        assert_eq!(
            serde_json::to_value(report).unwrap()["themeNotes"],
            serde_json::json!(["The theme defines no length tokens."])
        );
    }

    #[test]
    fn unmatched_lengths_are_info_and_exact_matches_remain_warnings() {
        let tsx = r##"export const X = () => <Box gap="40px" borderRadius="16px" />;"##;
        let report = validate_devup_ui_tsx(tsx, Some(&fixture_theme()), false);
        let lengths = report
            .violations
            .iter()
            .filter(|violation| violation.rule == "hardcoded-length")
            .collect::<Vec<_>>();
        // Both are visible; only 16px has actionable advice ($md).
        assert_eq!(lengths.len(), 2, "{lengths:?}");
        assert!(lengths[0].message.contains("40px"));
        assert_eq!(
            serde_json::to_value(lengths[0]).unwrap()["severity"],
            "info"
        );
        assert!(lengths[0].suggestion.is_none());
        assert!(lengths[1].message.contains("16px"));
        assert_eq!(lengths[1].severity, Severity::Warning);
        assert!(lengths[1].suggestion.as_ref().unwrap().contains("$md"));
    }

    /// Nonmatching colors remain visible without speculative token advice.
    #[test]
    fn unmatched_colors_are_info_even_in_strict_mode() {
        for strict in [false, true] {
            let report = validate_devup_ui_tsx(
                r##"export const X = () => <Box color="#752E2E" />;"##,
                Some(&fixture_theme()),
                strict,
            );
            assert!(report.ok);
            assert_eq!(report.violations.len(), 1);
            let finding = &report.violations[0];
            assert_eq!(finding.rule, "hardcoded-color");
            assert_eq!(serde_json::to_value(finding).unwrap()["severity"], "info");
            assert!(finding.message.contains("no matching token"));
            assert!(finding.suggestion.is_none());
        }
    }

    /// An unavailable theme cannot suppress literals or imply an empty theme.
    #[test]
    fn without_a_theme_both_hardcoded_rules_report_info_without_advice() {
        let report = validate_devup_ui_tsx(
            r##"export const X = () => <Box color="#999999" borderRadius="16px" />;"##,
            None,
            true,
        );
        assert!(report.ok);
        assert_eq!(report.violations.len(), 2);
        for finding in &report.violations {
            assert_eq!(serde_json::to_value(finding).unwrap()["severity"], "info");
            assert!(finding.suggestion.is_none());
            assert!(finding.message.contains("no theme is available"));
        }
        assert_eq!(
            serde_json::to_value(report).unwrap()["themeNotes"],
            serde_json::json!([])
        );
    }

    /// Missing categories are summarized once each, never on every literal.
    #[test]
    fn empty_color_and_length_categories_are_summarized_once_each() {
        let theme =
            parse_project_theme(r#"{"theme":{"colors":{"default":{}},"length":{}}}"#).unwrap();
        let report = validate_devup_ui_tsx(
            r##"export const X = () => <Box bg="#999999" color="#ffffff" p="8px" m="16px" />;"##,
            Some(&theme),
            true,
        );
        assert!(report.ok);
        assert_eq!(report.violations.len(), 4);
        let value = serde_json::to_value(report).unwrap();
        assert_eq!(
            value["themeNotes"],
            serde_json::json!([
                "The theme defines no color tokens.",
                "The theme defines no length tokens."
            ])
        );
        for finding in value["violations"].as_array().unwrap() {
            assert_eq!(finding["severity"], "info");
            assert!(!finding["message"].as_str().unwrap().contains("defines no"));
        }
    }

    #[test]
    fn a_reported_hardcoded_value_names_the_token_that_already_holds_it() {
        let tsx = r##"export const X = () => <Box color="#999999" borderRadius="16px" />;"##;
        let report = validate_devup_ui_tsx(tsx, Some(&fixture_theme()), false);
        let messages = report
            .violations
            .iter()
            .map(|violation| violation.message.clone())
            .collect::<Vec<_>>();
        assert!(
            messages
                .iter()
                .any(|message| message.contains("$captionLight")),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|message| message.contains("$md")),
            "{messages:?}"
        );
    }

    #[test]
    fn dynamic_jsx_props_are_not_flagged_as_runtime_value() {
        let tsx = r##"export const X = ({color}) => <Box bg={color} />;"##;
        let report = validate_devup_ui_tsx(tsx, Some(&fixture_theme()), false);
        assert!(
            report
                .violations
                .iter()
                .all(|violation| violation.rule != "runtime-value"),
            "{:?}",
            report.violations
        );
    }

    #[test]
    fn catches_runtime_value_inside_css_call() {
        let tsx = r##"
            import { css } from '@devup-ui/react'
            const v = getValue()
            const cls = css({ width: v })
        "##;
        let report = validate_devup_ui_tsx(tsx, Some(&fixture_theme()), false);
        assert!(!report.ok);
        assert!(
            report
                .violations
                .iter()
                .any(|violation| violation.rule == "runtime-value")
        );
    }

    #[test]
    fn allows_literal_only_css_call() {
        let tsx = r##"
            import { css } from '@devup-ui/react'
            const cls = css({ width: 1, height: '100%', items: [1, '2'] })
        "##;
        let report = validate_devup_ui_tsx(tsx, Some(&fixture_theme()), false);
        assert!(report.ok, "{:?}", report.violations);
    }

    #[test]
    fn flags_unknown_prop_on_primitive_element() {
        let tsx = r##"export const X = () => <Box notARealProp="x" />;"##;
        let report = validate_devup_ui_tsx(tsx, None, false);
        assert!(
            report
                .violations
                .iter()
                .any(|violation| violation.rule == "unknown-prop")
        );
    }

    #[test]
    fn does_not_flag_unknown_prop_on_custom_component() {
        let tsx = r##"export const X = () => <MyCustomWidget notARealProp="x" />;"##;
        let report = validate_devup_ui_tsx(tsx, None, false);
        assert!(
            report
                .violations
                .iter()
                .all(|violation| violation.rule != "unknown-prop"),
            "{:?}",
            report.violations
        );
    }

    #[test]
    fn does_not_flag_pseudo_and_event_props() {
        let tsx = r##"export const X = () => <Box _hover={{ bg: "red" }} onClick={fn} data-testid="x" as="button" />;"##;
        let report = validate_devup_ui_tsx(tsx, None, false);
        assert!(
            report
                .violations
                .iter()
                .all(|violation| violation.rule != "unknown-prop"),
            "{:?}",
            report.violations
        );
    }

    #[test]
    fn reports_invalid_syntax_as_violation_not_panic() {
        let report = validate_devup_ui_tsx("export const X = () => <Box bg=", None, false);
        assert!(!report.ok);
        assert!(
            report
                .violations
                .iter()
                .any(|violation| violation.rule == "invalid-syntax")
        );
    }

    #[test]
    fn missing_theme_skips_token_check_without_panicking() {
        let tsx = r##"export const X = () => <Box bg="$whateverToken" />;"##;
        let report = validate_devup_ui_tsx(tsx, None, false);
        assert_eq!(report.checked_tokens, 1);
        assert!(
            report
                .violations
                .iter()
                .all(|violation| violation.rule != "unknown-token")
        );
    }

    #[test]
    fn strict_mode_fails_on_warnings() {
        let tsx = r##"export const X = () => <Box color="#999999" />;"##;
        let lenient = validate_devup_ui_tsx(tsx, Some(&fixture_theme()), false);
        let strict = validate_devup_ui_tsx(tsx, Some(&fixture_theme()), true);
        assert!(lenient.ok);
        assert!(!strict.ok);
    }

    fn purpose_named_length_theme() -> ProjectTheme {
        parse_project_theme(
            r##"{ "theme": { "length": { "default": {
                "space0": "0px", "spacing24": "24px", "sizeStrokeBorder": "1px",
                "radius12": "12px", "space12": "12px"
            } } } }"##,
        )
        .unwrap()
    }

    #[test]
    fn zero_lengths_are_never_reported() {
        let tsx = r##"export const X = () => <Box left="0px" top="-0px" m="0.0px" />;"##;
        let report = validate_devup_ui_tsx(tsx, Some(&purpose_named_length_theme()), true);
        assert!(report.ok);
        assert!(
            report.violations.iter().all(|v| v.rule != "hardcoded-length"),
            "{:?}",
            report.violations
        );
    }

    #[test]
    fn a_token_named_for_another_purpose_is_info_without_advice() {
        let tsx = r##"export const X = () => <Box borderRadius="24px" h="1px" />;"##;
        let report = validate_devup_ui_tsx(tsx, Some(&purpose_named_length_theme()), true);
        assert!(report.ok, "{:?}", report.violations);
        let lengths = report
            .violations
            .iter()
            .filter(|v| v.rule == "hardcoded-length")
            .collect::<Vec<_>>();
        assert_eq!(lengths.len(), 2);
        for finding in lengths {
            assert_eq!(finding.severity, Severity::Info);
            assert!(finding.suggestion.is_none());
            assert!(finding.context.contains_key("otherPurposeTokens"));
            assert!(finding.message.contains("different purpose"));
        }
    }

    #[test]
    fn a_token_named_for_the_same_purpose_is_preferred() {
        let theme = purpose_named_length_theme();
        let radius = validate_devup_ui_tsx(
            r##"export const X = () => <Box borderRadius="12px" />;"##,
            Some(&theme),
            false,
        );
        let gap = validate_devup_ui_tsx(
            r##"export const X = () => <Box gap="12px" />;"##,
            Some(&theme),
            false,
        );
        let radius = &radius.violations[0];
        let gap = &gap.violations[0];
        assert_eq!(radius.severity, Severity::Warning);
        assert_eq!(gap.severity, Severity::Warning);
        assert!(radius.message.contains("$radius12") && !radius.message.contains("$space12"));
        assert!(gap.message.contains("$space12") && !gap.message.contains("$radius12"));
    }

    #[test]
    fn mode_advice_is_stated_once() {
        let report = validate_devup_ui_tsx(
            r##"export const X = () => <Box borderRadius="16px" />;"##,
            Some(&fixture_theme()),
            false,
        );
        let finding = &report.violations[0];
        assert!(!finding.message.contains("mode"));
        assert!(finding.suggestion.as_deref().unwrap().contains("allModesMatch"));
        assert_eq!(finding.context["value"], "16px");
        assert_eq!(finding.context["prop"], "borderRadius");
    }
}

#[path = "../ui_structure.rs"]
mod structure;
pub use structure::{
    MAX_FILES, MAX_TOTAL_BYTES, SourceFile, bundle_theme, check_bundle, validate_bundle, validate_sources,
};
