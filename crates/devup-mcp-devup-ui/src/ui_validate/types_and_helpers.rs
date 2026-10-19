
use std::collections::BTreeSet;
#[path = "blur_layers.rs"]
mod blur_layers;
#[path = "site.rs"]
pub mod site;

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, CallExpression, Expression, JSXAttribute, JSXAttributeItem, JSXAttributeName,
    JSXAttributeValue, JSXElementName, JSXOpeningElement, ObjectExpression, ObjectPropertyKind,
    PropertyKey, UnaryOperator,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};
use serde::Serialize;

use crate::style_props::{
    DEVUP_PRIMITIVE_ELEMENTS, is_color_like_prop, is_known_non_style_prop, is_known_style_prop,
    is_length_like_prop,
};
use crate::theme::{ProjectTheme, TokenCategory, closest_tokens};

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
    #[serde(flatten)]
    pub context: serde_json::Map<String, serde_json::Value>,
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
    validate_source(tsx, theme, strict, SourceType::tsx())
}

fn validate_source(
    tsx: &str,
    theme: Option<&ProjectTheme>,
    strict: bool,
    source_type: SourceType,
) -> UiValidation {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, tsx, source_type).parse();

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
            context: Default::default(),
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
        render_element_stack: Vec::new(),
    };
    visitor.visit_program(&parsed.program);
    violations.extend(visitor.violations);
    let checked_tokens = visitor.checked_tokens;
    let attributes = crate::provenance::attributes::generated_attributes(tsx);
    violations.extend(blur_layers::check(tsx, &attributes));
    for risk in crate::ui_layout::layout_risks(tsx, None)["risks"].as_array().into_iter().flatten() {
        let rule = match risk["rule"].as_str() {
            Some("narrow-fixed-columns") => "narrow-fixed-columns",
            Some("column-reading-order") => "column-reading-order",
            _ => continue,
        };
        violations.push(Violation { rule, severity: Severity::Warning, byte_range: [0, 0],
            context: risk.as_object().cloned().unwrap_or_default(),
            message: risk["message"].as_str().unwrap_or_default().to_owned(),
            suggestion: Some("Use reading-order DOM and explicit narrow layout props, then run the runtime audit at every required width.".into()) });
    }

    let elements = crate::provenance::attributes::opening_elements(tsx);
    for violation in &mut violations {
        let [start, end] = violation.byte_range;
        if let Some(attribute) = attributes
            .iter()
            .find(|attribute| attribute.start <= start && end <= attribute.end)
        {
            violation
                .context
                .insert("elementIndex".into(), serde_json::json!(attribute.element));
            violation
                .context
                .insert("element".into(), serde_json::json!(attribute.tag));
            violation
                .context
                .insert("attribute".into(), serde_json::json!(attribute.name));
        } else if let Some(element) = elements
            .iter()
            .filter(|element| element.start <= start && end <= element.end)
            .min_by_key(|element| element.end - element.start)
        {
            violation
                .context
                .insert("elementIndex".into(), serde_json::json!(element.ordinal));
        }
        let prefix = tsx.get(..start).unwrap_or("");
        violation.context.insert(
            "line".into(),
            serde_json::json!(prefix.bytes().filter(|b| *b == b'\n').count() + 1),
        );
        violation.context.insert(
            "column".into(),
            serde_json::json!(prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1),
        );
        violation.context.insert(
            "positionEncoding".into(),
            serde_json::json!("1-based Unicode scalar columns; byteRange uses UTF-8 bytes"),
        );
        violation.context.insert(
            "snippet".into(),
            serde_json::json!(
                tsx.get(start..end)
                    .unwrap_or("")
                    .chars()
                    .take(120)
                    .collect::<String>()
            ),
        );
    }

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
    render_element_stack: Vec<Option<String>>,
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
                let expected = token_category_for_prop(prop_name);
                let names = catalog
                    .iter()
                    .filter(|(_, entry)| expected.is_none_or(|category| entry.category == category))
                    .map(|(name, _)| name)
                    .collect::<Vec<_>>();
                let suggestions = closest_tokens(token, names.into_iter(), 3);
                self.violations.push(Violation {
                    rule: "unknown-token",
                    severity: Severity::Error,
                    context: Default::default(),
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
        // devup-ui's `typography` prop names a theme token without a `$`, so
        // the check above never sees one. A generated Braillify Studio screen
        // carried `typography="mainSubText"`, `"titleSmMed"` and `"bodyMed"`,
        // none of them in that project's devup.json, and validation reported
        // six unknown tokens — every one a color — while these three passed.
        // An unknown typography token is a style that silently does not apply.
        if prop_name == "typography" {
            let Some(theme) = self.theme else {
                return;
            };
            // Nothing to check against, and a check that cannot be made is
            // skipped rather than guessed, as it is for an absent theme.
            if theme.typography.is_empty() {
                return;
            }
            self.checked_tokens += 1;
            if theme.typography.contains_key(text) {
                return;
            }
            let suggestions = closest_tokens(text, theme.typography.keys(), 3);
            self.violations.push(Violation {
                rule: "unknown-token",
                severity: Severity::Error,
                context: Default::default(),
                byte_range: [span.start as usize, span.end as usize],
                message: format!("{text} is not a typography token in devup.json."),
                suggestion: if suggestions.is_empty() {
                    None
                } else {
                    Some(format!(
                        "closest existing typography tokens: {}",
                        suggestions.join(", ")
                    ))
                },
            });
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
                context: Default::default(),
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
        let token_matches: Vec<_> = tokens.iter().map(|name| {
            let map = if rule == "hardcoded-color" { &self.theme.unwrap().colors } else { &self.theme.unwrap().length };
            let matching_modes: Vec<_> = map.iter().filter(|(_, values)| values.get(name).and_then(|v| v.as_str()).is_some_and(|v| if rule == "hardcoded-color" { v.trim().eq_ignore_ascii_case(text.trim()) } else { v == text })).map(|(mode,_)|mode.clone()).collect();
            serde_json::json!({"token":format!("${name}"),"matchingModes":matching_modes,"allModesMatch":!map.is_empty() && matching_modes.len() == map.len()})
        }).collect();
        self.violations.push(Violation {
            rule,
            severity: Severity::Warning,
            context: serde_json::Map::from_iter([("tokenMatches".into(), serde_json::json!(token_matches))]),
            byte_range: [span.start as usize, span.end as usize],
            message: format!(
                include_str!("../text/ui_validate-report_hardcoded_value_Violation-63c589c5.md")
            , kind = kind, named = named, prop_name = prop_name, text = text),
            suggestion: Some(format!("matching tokens: {named}. Check tokenMatches.matchingModes and allModesMatch before replacing; a partial-mode match can change colors in other modes.")),
        });
    }

    fn check_unknown_prop(&mut self, prop_name: &str, span: Span) {
        if !self.current_is_primitive() {
            return;
        }
        if is_known_style_prop(prop_name) || is_known_non_style_prop(prop_name) {
            return;
        }
        if self
            .render_element_stack
            .last()
            .and_then(|tag| tag.as_deref())
            .is_some_and(|tag| crate::html_props::is_html_prop(tag, prop_name))
        {
            return;
        }
        self.violations.push(Violation {
            rule: "unknown-prop",
            severity: Severity::Error,
            context: Default::default(),
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
        self.check_object_tokens(object);
    }

    /// `$token` references inside `css()`/`globalCss()`/`keyframes()` object
    /// literals.
    ///
    /// The token check used to read JSX attributes only, so
    /// `globalCss({ body: { background: "$bg" } })` reported
    /// `checkedTokens: 0`: a token that did not exist was accepted in
    /// silence, and the response said `clean` about code referencing a token
    /// nobody had verified. Only `$`-prefixed strings are inspected here —
    /// the hardcoded-value advice stays on JSX props, so no call gains a new
    /// warning severity from this walk.
    fn check_object_tokens(&mut self, object: &ObjectExpression) {
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                continue;
            };
            let key = property_key_name(&property.key).unwrap_or_else(|| "?".to_owned());
            self.check_value_tokens(&key, &property.value);
        }
    }

    /// Nested objects carry the selector/breakpoint nesting devup-ui allows,
    /// and arrays carry responsive values; the key that decides a token's
    /// expected category is the innermost one, which is the CSS property.
    fn check_value_tokens(&mut self, key: &str, value: &Expression) {
        match value {
            Expression::StringLiteral(text) if text.value.starts_with('$') => {
                self.check_attribute_value(key, text.value.as_str(), text.span);
            }
            Expression::ObjectExpression(nested) => self.check_object_tokens(nested),
            Expression::ArrayExpression(array) => {
                for element in &array.elements {
                    if let Some(expression) = element.as_expression() {
                        self.check_value_tokens(key, expression);
                    }
                }
            }
            _ => {}
        }
    }

    fn check_static_object(&mut self, object: &ObjectExpression, call_name: &str) {
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                continue;
            };
            let key = property_key_name(&property.key).unwrap_or_else(|| "?".to_owned());
            if !is_static_expression(&property.value, call_name == "css") {
                self.violations.push(Violation {
                    rule: "runtime-value",
                    severity: Severity::Error,
                    context: Default::default(),
                    byte_range: [
                        property.value.span().start as usize,
                        property.value.span().end as usize,
                    ],
                    message: format!(
                        include_str!("../text/ui_validate-check_static_object_Violation-a031fb84.md"),
                        call_name = call_name,
                        key = key
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
        // Resolve before visiting attributes so maxLength before as works too.
        // A later spread/dynamic as can override a literal; do not assume its tag.
        let mut rendered = match tag_name.as_deref() {
            Some("Text") => Some("span".to_owned()),
            Some("Image") => Some("img".to_owned()),
            Some("Box" | "Flex" | "Center" | "Grid") => Some("div".to_owned()),
            _ => None,
        };
        for attribute in &element.attributes {
            match attribute {
                JSXAttributeItem::SpreadAttribute(_) => rendered = None,
                JSXAttributeItem::Attribute(attribute) => {
                    if matches!(&attribute.name, JSXAttributeName::Identifier(name) if name.name == "as")
                    {
                        rendered = match &attribute.value {
                            Some(JSXAttributeValue::StringLiteral(value)) => {
                                Some(value.value.to_string())
                            }
                            Some(JSXAttributeValue::ExpressionContainer(container)) => container
                                .expression
                                .as_expression()
                                .and_then(|expression| match expression.get_inner_expression() {
                                    Expression::StringLiteral(value) => {
                                        Some(value.value.to_string())
                                    }
                                    _ => None,
                                }),
                            _ => None,
                        };
                    }
                }
            }
        }
        self.element_stack.push(tag_name);
        self.render_element_stack.push(rendered);
        walk::walk_jsx_opening_element(self, element);
        self.render_element_stack.pop();
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
/// substitutions, and arbitrary runtime value leaves are not. css() alone can
/// select among recursively static conditional branches; the condition is kept
/// in generated class selection (devup-ui extractor::extract_style_from_expression).
fn is_static_expression(expression: &Expression, conditional_classes: bool) -> bool {
    match expression.get_inner_expression() {
        Expression::Identifier(id) if conditional_classes && id.name == "undefined" => true,
        Expression::ConditionalExpression(conditional) => {
            conditional_classes
                && is_static_expression(&conditional.consequent, true)
                && is_static_expression(&conditional.alternate, true)
        }
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_) => true,
        Expression::TemplateLiteral(template) => template.expressions.is_empty(),
        Expression::UnaryExpression(unary) => {
            matches!(
                unary.operator,
                UnaryOperator::UnaryNegation | UnaryOperator::UnaryPlus
            ) && is_static_expression(&unary.argument, conditional_classes)
        }
        Expression::ArrayExpression(array) => array.elements.iter().all(|element| {
            element
                .as_expression()
                .is_some_and(|e| is_static_expression(e, conditional_classes))
                || element.is_elision()
        }),
        Expression::ObjectExpression(object) => {
            object.properties.iter().all(|property| match property {
                ObjectPropertyKind::ObjectProperty(property) => {
                    is_static_expression(&property.value, conditional_classes)
                }
                ObjectPropertyKind::SpreadProperty(_) => false,
            })
        }
        _ => false,
    }
}

/// Which category a `$token` written in this prop could belong to.
///
/// A prop admits one kind of token, so only that kind can replace a name
/// the theme does not have. Ranking the whole catalog by edit distance
/// ignores that and answers a color prop with whatever is spelled closest:
/// `bg="$background"` came back as `$bodyStrong, $button, $caption`, three
/// typography tokens, while `$bg` — the color the project defines for
/// exactly this — was never offered.
///
/// `None` for a prop this validator does not model. The whole catalog is
/// ranked then, as before: a worse suggestion is still better than none,
/// and narrowing on a guess would be the same mistake in reverse.
fn token_category_for_prop(prop_name: &str) -> Option<TokenCategory> {
    if is_color_like_prop(prop_name) {
        Some(TokenCategory::Colors)
    } else if is_length_like_prop(prop_name) {
        Some(TokenCategory::Length)
    } else if matches!(prop_name, "boxShadow" | "textShadow") {
        Some(TokenCategory::Shadow)
    } else {
        None
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