//! `devup_ui_validate` — the highest-leverage of the three ground-truth
//! tools. Parses TSX with the same `oxc_parser`/`oxc_allocator`/`oxc_span`
//! stack already used to validate every generated TSX (`validation.rs`),
//! then walks the AST with `oxc_ast_visit::Visit` to catch the exact
//! failure class documented in this repository's brief: three agents
//! independently inventing `$gray100` (a color token that does not exist
//! in the project's real `devup.json`), a 16px bubble radius, and a 36px
//! avatar size, none traceable to any source of truth.
//!
//! Two facts verified against `@devup-ui/react`'s own docs and ESLint rule
//! (`css-utils-literal-only`) shape the rules here and deliberately
//! *narrow* what the brief's "런타임 값" wording might suggest:
//!
//! - JSX style props on `Box`/`Flex`/`Text`/... (`bg={dynamicValue}`) ARE
//!   valid devup-ui: the compiler lowers them to a CSS custom property at
//!   build time (`className="a" style={{"--a": dynamicValue}}`). Flagging
//!   these as errors would itself be a fabricated rule.
//! - `css()`, `globalCss()`, and `keyframes()` utility calls are the actual
//!   "must be statically analyzable" boundary — devup-ui's own
//!   `css-utils-literal-only` ESLint rule rejects variables/expressions
//!   there, because these calls are extracted at build time with no
//!   runtime fallback. `runtime-value` therefore targets these three call
//!   sites, not general JSX props.
//!
//! `unknown-token` / `hardcoded-color` / `hardcoded-length` operate on the
//! `Box`/`Flex`/`Text`/`Center`/`Grid`/`Image` primitives' known color- and
//! length-like props (`style_props.rs`, itself sourced from devup-ui's
//! published Style Props API reference, not invented).
//!
//! The two `hardcoded-*` rules always report literals. Exact token matches
//! are warnings with actionable token advice; unmatched values are info
//! without token suggestions. Empty token categories are summarized once
//! in `themeNotes`, avoiding repeated, impossible advice (D11).

use std::collections::BTreeSet;

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, CallExpression, Expression, JSXAttribute, JSXAttributeName, JSXAttributeValue,
    JSXElementName, JSXOpeningElement, ObjectExpression, ObjectPropertyKind, PropertyKey,
    UnaryOperator,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};
use serde::Serialize;

use crate::style_props::{
    DEVUP_PRIMITIVE_ELEMENTS, is_color_like_prop, is_known_non_style_prop, is_known_style_prop,
    is_length_like_prop,
};
use crate::theme::{ProjectTheme, closest_tokens};

/// Devup-ui `css`/`globalCss`/`keyframes` utility call names whose object
/// argument must be statically analyzable (devup-ui's own
/// `css-utils-literal-only` ESLint rule constraint).
const LITERAL_ONLY_CALLS: &[&str] = &["css", "globalCss", "keyframes"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Violation {
    pub rule: &'static str,
    pub severity: Severity,
    pub byte_range: [usize; 2],
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

/// The result of one validation, and what each field does and does not
/// claim. The MCP tool publishes these four under the same names
/// (`ok`, `violations`, `checkedTokens`, `availableTokenCount`), and a
/// caller who reads them as anything other than the below will draw the
/// wrong conclusion — `ok: true` beside a non-empty `violations` is
/// correct output, not a contradiction.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiValidation {
    /// Whether this TSX may be used as it stands.
    ///
    /// `false` when any violation is [`Severity::Error`] — a syntax
    /// failure, a `$token` the theme does not define, a prop the
    /// primitive does not take, or a runtime value where devup-ui
    /// extracts at build time. Each of those breaks the build or the
    /// screen.
    ///
    /// `true` is therefore compatible with a non-empty `violations`:
    /// warnings are values that would be better written as a token, and
    /// code that ignores every one of them still compiles and renders.
    /// Pass `strict` to fail on warnings too. Info alone never fails `ok`.
    pub ok: bool,
    /// Every finding, in source order per rule. `severity` is what
    /// decides `ok`; the count alone does not.
    pub violations: Vec<Violation>,
    /// One note per empty token category encountered by the hardcoded rules.
    /// An unavailable theme is handled by the caller's guardrail instead.
    pub theme_notes: Vec<String>,
    /// How many `$token` references this TSX makes — i.e. how many string
    /// attribute values began with `$` and were looked up. It counts the
    /// *code's* references, not the theme's tokens, and it counts them
    /// whether or not a theme was available to check them against: with
    /// no theme, `checked_tokens` is still the number of references while
    /// `unknown-token` is skipped entirely (a skipped check must not read
    /// as a passed one).
    pub checked_tokens: usize,
    /// How many distinct tokens the project's `devup.json` defines across
    /// all four axes, merged by name — the size of the vocabulary the
    /// TSX had to choose from. `0` when no theme was available.
    ///
    /// Not comparable with `checked_tokens`: one counts references in the
    /// code, the other definitions in the theme.
    pub available_token_count: usize,
}

/// Validates `tsx` against `theme` (a project's real `devup.json`, or
/// `None` if unavailable — in which case `unknown-token` is skipped rather
/// than guessed at; callers should surface `theme` unavailability to the
/// user separately, since silently skipping token checks is different from
/// confirming a token exists). `strict` additionally fails `ok` on
/// `warning`-severity violations.
pub fn validate_devup_ui_tsx(
    tsx: &str,
    theme: Option<&ProjectTheme>,
    strict: bool,
) -> UiValidation {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, tsx, SourceType::tsx()).parse();

    let mut violations = Vec::new();
    for diagnostic in &parsed.diagnostics {
        let (start, end) = diagnostic
            .labels
            .first()
            .map(|label| {
                let start = (label.offset() as usize).min(tsx.len());
                let end = start.saturating_add(label.len() as usize).min(tsx.len());
                (start, end)
            })
            .unwrap_or((0, 0));
        violations.push(Violation {
            rule: "invalid-syntax",
            severity: Severity::Error,
            byte_range: [start, end],
            message: format!("TSX failed TypeScript+JSX syntax validation: {diagnostic}"),
            suggestion: None,
        });
    }

    let available_token_count = theme.map(ProjectTheme::token_count).unwrap_or(0);
    let mut visitor = TsxVisitor {
        theme,
        checked_tokens: 0,
        violations: Vec::new(),
        element_stack: Vec::new(),
    };
    visitor.visit_program(&parsed.program);
    violations.extend(visitor.violations);
    let checked_tokens = visitor.checked_tokens;

    let ok = violations.iter().all(|violation| {
        violation.severity != Severity::Error
            && !(strict && violation.severity == Severity::Warning)
    });
    let theme_notes = theme
        .map(|theme| {
            [
                ("hardcoded-color", "color", &theme.colors),
                ("hardcoded-length", "length", &theme.length),
            ]
            .into_iter()
            .filter(|(rule, _, modes)| {
                modes.values().all(|tokens| tokens.is_empty())
                    && violations.iter().any(|finding| finding.rule == *rule)
            })
            .map(|(_, kind, _)| format!("The theme defines no {kind} tokens."))
            .collect()
        })
        .unwrap_or_default();

    UiValidation {
        ok,
        violations,
        theme_notes,
        checked_tokens,
        available_token_count,
    }
}

struct TsxVisitor<'t> {
    theme: Option<&'t ProjectTheme>,
    checked_tokens: usize,
    violations: Vec<Violation>,
    element_stack: Vec<Option<String>>,
}

impl<'t> TsxVisitor<'t> {
    fn current_is_primitive(&self) -> bool {
        self.element_stack
            .last()
            .and_then(|name| name.as_deref())
            .is_some_and(|name| DEVUP_PRIMITIVE_ELEMENTS.contains(&name))
    }

    fn check_attribute_value(&mut self, prop_name: &str, text: &str, span: Span) {
        if let Some(token) = text.strip_prefix('$') {
            self.checked_tokens += 1;
            if let Some(theme) = self.theme
                && !theme.contains_token(token)
            {
                let catalog = theme.token_catalog();
                let names = catalog.keys().collect::<Vec<_>>();
                let suggestions = closest_tokens(token, names.into_iter(), 3);
                self.violations.push(Violation {
                    rule: "unknown-token",
                    severity: Severity::Error,
                    byte_range: [span.start as usize, span.end as usize],
                    message: format!("${token} is not defined in devup.json."),
                    suggestion: if suggestions.is_empty() {
                        None
                    } else {
                        Some(format!(
                            "closest existing tokens: {}",
                            suggestions
                                .iter()
                                .map(|name| format!("${name}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ))
                    },
                });
            }
            return;
        }
        if is_color_like_prop(prop_name) && is_hex_color(text) {
            let tokens = self
                .theme
                .map(|theme| theme.color_tokens_matching_hex(text))
                .unwrap_or_default();
            self.report_hardcoded_value("hardcoded-color", "color", prop_name, text, &tokens, span);
            return;
        }
        if is_length_like_prop(prop_name) && is_px_length(text) {
            let tokens = self
                .theme
                .map(|theme| theme.length_tokens_matching_px(text))
                .unwrap_or_default();
            self.report_hardcoded_value(
                "hardcoded-length",
                "length",
                prop_name,
                text,
                &tokens,
                span,
            );
        }
    }

    /// Every literal remains visible. Only exact matches justify a warning
    /// and token advice; unmatched values carry factual info with no suggestion.
    fn report_hardcoded_value(
        &mut self,
        rule: &'static str,
        kind: &str,
        prop_name: &str,
        text: &str,
        tokens: &[String],
        span: Span,
    ) {
        if tokens.is_empty() {
            let context = if self.theme.is_some() {
                "the theme has no matching token"
            } else {
                "no theme is available to check for a matching token"
            };
            self.violations.push(Violation {
                rule,
                severity: Severity::Info,
                byte_range: [span.start as usize, span.end as usize],
                message: format!("{prop_name} uses hardcoded {kind} {text}; {context}."),
                suggestion: None,
            });
            return;
        }
        let named = tokens
            .iter()
            .map(|name| format!("${name}"))
            .collect::<Vec<_>>()
            .join(", ");
        self.violations.push(Violation {
            rule,
            severity: Severity::Warning,
            byte_range: [span.start as usize, span.end as usize],
            message: format!(
                "{prop_name} uses hardcoded {kind} {text}, which this project's devup.json already defines as {named}. Use the token so a theme change reaches this value."
            ),
            suggestion: Some(format!("matching tokens: {named}")),
        });
    }

    fn check_unknown_prop(&mut self, prop_name: &str, span: Span) {
        if !self.current_is_primitive() {
            return;
        }
        if is_known_style_prop(prop_name) || is_known_non_style_prop(prop_name) {
            return;
        }
        self.violations.push(Violation {
            rule: "unknown-prop",
            severity: Severity::Error,
            byte_range: [span.start as usize, span.end as usize],
            message: format!(
                "{prop_name} is not a prop recognized by {}.",
                self.element_stack
                    .last()
                    .and_then(|name| name.as_deref())
                    .unwrap_or("devup-ui primitive")
            ),
            suggestion: None,
        });
    }

    fn check_literal_only_call(&mut self, call: &CallExpression) {
        let Some(callee) = call.callee.get_identifier_reference() else {
            return;
        };
        if !LITERAL_ONLY_CALLS.contains(&callee.name.as_str()) {
            return;
        }
        let Some(Argument::ObjectExpression(object)) = call.arguments.first() else {
            return;
        };
        self.check_static_object(object, callee.name.as_str());
    }

    fn check_static_object(&mut self, object: &ObjectExpression, call_name: &str) {
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                continue;
            };
            let key = property_key_name(&property.key).unwrap_or_else(|| "?".to_owned());
            if !is_static_expression(&property.value) {
                self.violations.push(Violation {
                    rule: "runtime-value",
                    severity: Severity::Error,
                    byte_range: [
                        property.value.span().start as usize,
                        property.value.span().end as usize,
                    ],
                    message: format!(
                        "{call_name}({{ {key}: ... }}) accepts only statically analyzable literal values. Variables or expressions break zero-runtime extraction."
                    ),
                    suggestion: None,
                });
            }
        }
    }
}

impl<'a, 't> Visit<'a> for TsxVisitor<'t> {
    fn visit_jsx_opening_element(&mut self, element: &JSXOpeningElement<'a>) {
        let tag_name = jsx_element_name(&element.name);
        self.element_stack.push(tag_name);
        walk::walk_jsx_opening_element(self, element);
        self.element_stack.pop();
    }

    fn visit_jsx_attribute(&mut self, attribute: &JSXAttribute<'a>) {
        if let JSXAttributeName::Identifier(name) = &attribute.name {
            let prop_name = name.name.as_str();
            self.check_unknown_prop(prop_name, name.span);
            if let Some(JSXAttributeValue::StringLiteral(literal)) = &attribute.value {
                self.check_attribute_value(prop_name, literal.value.as_str(), literal.span);
            }
        }
        walk::walk_jsx_attribute(self, attribute);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        self.check_literal_only_call(call);
        walk::walk_call_expression(self, call);
    }
}

fn jsx_element_name(name: &JSXElementName) -> Option<String> {
    match name {
        JSXElementName::Identifier(identifier) => Some(identifier.name.as_str().to_owned()),
        JSXElementName::IdentifierReference(reference) => Some(reference.name.as_str().to_owned()),
        _ => None,
    }
}

fn property_key_name(key: &PropertyKey) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(identifier) => Some(identifier.name.as_str().to_owned()),
        PropertyKey::StringLiteral(literal) => Some(literal.value.as_str().to_owned()),
        _ => None,
    }
}

/// Static-analysis literal check mirroring devup-ui's `css-utils-literal-only`
/// ESLint rule: string/number/boolean/null literals, unary-negated numeric
/// literals, and arrays/objects composed entirely of such, are allowed.
/// Identifiers, member/call expressions, template literals with
/// substitutions, and any other runtime-dependent expression are not.
fn is_static_expression(expression: &Expression) -> bool {
    match expression {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_) => true,
        Expression::TemplateLiteral(template) => template.expressions.is_empty(),
        Expression::UnaryExpression(unary) => {
            matches!(
                unary.operator,
                UnaryOperator::UnaryNegation | UnaryOperator::UnaryPlus
            ) && is_static_expression(&unary.argument)
        }
        Expression::ArrayExpression(array) => array.elements.iter().all(|element| {
            element.as_expression().is_some_and(is_static_expression) || element.is_elision()
        }),
        Expression::ObjectExpression(object) => {
            object.properties.iter().all(|property| match property {
                ObjectPropertyKind::ObjectProperty(property) => {
                    is_static_expression(&property.value)
                }
                ObjectPropertyKind::SpreadProperty(_) => false,
            })
        }
        _ => false,
    }
}

fn is_hex_color(text: &str) -> bool {
    let Some(hex) = text.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|character| character.is_ascii_hexdigit())
}

fn is_px_length(text: &str) -> bool {
    let Some(number) = text.strip_suffix("px") else {
        return false;
    };
    let number = number.strip_prefix('-').unwrap_or(number);
    !number.is_empty()
        && number
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.')
        && number.matches('.').count() <= 1
}

/// All prop-name-independent identifiers this validator can flag, exposed
/// for tests that want to assert coverage without duplicating the rule
/// list.
pub fn rule_names() -> BTreeSet<&'static str> {
    [
        "invalid-syntax",
        "unknown-token",
        "hardcoded-color",
        "hardcoded-length",
        "unknown-prop",
        "runtime-value",
    ]
    .into_iter()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::parse_project_theme;

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
}
