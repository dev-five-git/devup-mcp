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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiValidation {
    pub ok: bool,
    pub violations: Vec<Violation>,
    pub checked_tokens: usize,
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
            message: format!("TSX가 TypeScript+JSX 문법 검증을 통과하지 못했습니다: {diagnostic}"),
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

    let ok = violations
        .iter()
        .all(|violation| violation.severity != Severity::Error)
        && (!strict || violations.is_empty());

    UiValidation {
        ok,
        violations,
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
                    message: format!("${token}은(는) devup.json에 정의되어 있지 않습니다."),
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
            let suggestion = self
                .theme
                .map(|theme| theme.color_tokens_matching_hex(text));
            self.violations.push(Violation {
                rule: "hardcoded-color",
                severity: Severity::Warning,
                byte_range: [span.start as usize, span.end as usize],
                message: format!(
                    "{prop_name}에 하드코딩된 색상 {text}을(를) 사용했습니다. devup.json 토큰 사용을 고려하세요."
                ),
                suggestion: match suggestion {
                    Some(tokens) if !tokens.is_empty() => Some(format!(
                        "matching tokens: {}",
                        tokens
                            .iter()
                            .map(|name| format!("${name}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                    _ => None,
                },
            });
            return;
        }
        if is_length_like_prop(prop_name) && is_px_length(text) {
            let suggestion = self
                .theme
                .map(|theme| theme.length_tokens_matching_px(text));
            self.violations.push(Violation {
                rule: "hardcoded-length",
                severity: Severity::Warning,
                byte_range: [span.start as usize, span.end as usize],
                message: format!(
                    "{prop_name}에 하드코딩된 길이 {text}을(를) 사용했습니다. devup.json 토큰 사용을 고려하세요."
                ),
                suggestion: match suggestion {
                    Some(tokens) if !tokens.is_empty() => Some(format!(
                        "matching tokens: {}",
                        tokens
                            .iter()
                            .map(|name| format!("${name}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                    _ => None,
                },
            });
        }
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
                "{prop_name}은(는) {}이(가) 인식하는 prop이 아닙니다.",
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
                        "{call_name}({{ {key}: ... }})는 정적으로 분석 가능한 리터럴 값만 허용합니다. 변수나 표현식은 zero-runtime 추출을 깨뜨립니다."
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
