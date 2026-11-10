//! Source quality checks independent of whether the agent installed skills.
use super::{Severity, Violation, site};
use crate::style_props::is_known_style_prop;
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use std::collections::BTreeMap;

pub(super) fn findings(source: &str) -> Vec<Violation> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    if parsed.panicked || !parsed.diagnostics.is_empty() { return Vec::new(); }
    let mut visitor = Collector { source, findings: Vec::new() };
    for comment in &parsed.program.comments {
        if source[comment.span.start as usize..comment.span.end as usize].contains("@devup-") {
            visitor.findings.push(site::violation(source, comment.span, "tsx-metadata-comment", Severity::Error,
                "Application TSX contains devup export metadata comments.".into(),
                "Keep interaction/override evidence in JSON sidecars and reflect required overrides in real props."));
        }
    }
    visitor.visit_program(&parsed.program);
    visitor.findings
}

struct Collector<'a> { source: &'a str, findings: Vec<Violation> }

fn literal(attribute: &JSXAttribute<'_>, source: &str) -> Option<String> {
    match attribute.value.as_ref()? {
        JSXAttributeValue::StringLiteral(value) => Some(value.value.to_string()),
        JSXAttributeValue::ExpressionContainer(container) => {
            let expression = container.expression.as_expression()?;
            if let Expression::StringLiteral(value) = expression { return Some(value.value.to_string()); }
            if static_expression(expression) {
                let span = expression.span();
                Some(source[span.start as usize..span.end as usize].to_owned())
            } else { None }
        }
        _ => None,
    }
}

fn static_expression(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::StringLiteral(_) | Expression::NumericLiteral(_) | Expression::NullLiteral(_) | Expression::BooleanLiteral(_) => true,
        Expression::ArrayExpression(array) => array.elements.iter().all(|item| item.as_expression().is_some_and(static_expression)),
        _ => false,
    }
}

impl<'a> Visit<'a> for Collector<'_> {
    fn visit_jsx_opening_element(&mut self, node: &JSXOpeningElement<'a>) {
        let mut props = BTreeMap::new();
        let mut spread = false;
        for item in &node.attributes {
            let JSXAttributeItem::Attribute(attribute) = item else { spread = true; continue };
            let JSXAttributeName::Identifier(name) = &attribute.name else { continue };
            let name = name.name.as_str();
            if matches!(name, "data-figma-node-id" | "data-figma-node-ids" | "data-source-node-id" | "data-source-node-ids") {
                self.findings.push(site::violation(self.source, attribute.span, "tsx-node-identifier", Severity::Error,
                    "Figma identity is embedded in application markup.".into(), "Use source-map sidecars and semantic runtime targets; do not add a strip plugin or debug exception."));
            }
            if let Some(value) = literal(attribute, self.source) {
                if is_known_style_prop(name) && value.contains("var(--") {
                    self.findings.push(site::violation(self.source, attribute.span, "jsx-token-syntax", Severity::Warning,
                        "A JSX style uses raw CSS variable syntax.".into(), "Use an existing $token in JSX, including arrays; var(--token) belongs in external CSS objects. Verify every breakpoint's computed styles."));
                }
                if matches!(name, "src" | "srcSet" | "poster" | "maskImage" | "backgroundImage")
                    && ["/", "url(/", "url('/", "url(\"/"].iter().any(|prefix| value.starts_with(prefix))
                    && (value.contains("/icons/figma/") || value.contains("/images/web-ready/")
                    || value.split('/').any(|part| part.split('.').next().is_some_and(|stem| stem.len()>=24 && stem.bytes().all(|b| b.is_ascii_hexdigit())))) {
                    self.findings.push(site::violation(self.source, attribute.span, "opaque-public-asset", Severity::Warning,
                        "An asset URL contains an opaque export identity.".into(), "Use a shared semantic path from the project asset registry; migrate code, seeds and persisted URLs together."));
                }
                props.insert(name, (value, attribute.span));
            }
        }
        if !spread {
            for (left, right, axis, aliases) in [
                ("pl", "pr", "px", ["p", "px", "padding", "paddingX"]),
                ("pt", "pb", "py", ["p", "py", "padding", "paddingY"]),
                ("ml", "mr", "mx", ["m", "mx", "margin", "marginX"]),
                ("mt", "mb", "my", ["m", "my", "margin", "marginY"]),
            ] {
                if !aliases.iter().any(|key| props.contains_key(key))
                    && let (Some((a, span)), Some((b, _))) = (props.get(left), props.get(right))
                    && a == b {
                    self.findings.push(site::violation(self.source, *span, "axis-shorthand", Severity::Warning,
                        format!("{left}/{right} repeat the same value or responsive array."), &format!("Use {axis} when no spread or longhand override changes precedence.")));
                }
            }
        }
        if let Some((background, _)) = props.get("bg").or_else(|| props.get("backgroundColor"))
            && background.starts_with("$secondary") {
            for item in &node.attributes {
                if let JSXAttributeItem::Attribute(attribute) = item
                    && let JSXAttributeName::Identifier(name) = &attribute.name
                    && matches!(name.name.as_str(), "_hover" | "_active" | "_focusVisible") {
                    let span = attribute.span;
                    if self.source[span.start as usize..span.end as usize].contains("$primary") {
                        self.findings.push(site::violation(self.source, span, "variant-state-palette", Severity::Warning,
                            "A secondary colour variant retains primary pseudo-state paint.".into(), "Review the captured variant palette for background, border, text and icon; never invent matching suffix tokens."));
                    }
                }
            }
        }
        if props.get("opacity").is_some_and(|(value, _)| value=="0")
            && node.attributes.iter().any(|attribute| matches!(attribute, JSXAttributeItem::Attribute(attribute) if matches!(&attribute.name, JSXAttributeName::Identifier(name) if name.name=="data-reveal"))) {
            self.findings.push(site::violation(self.source, node.span, "ssr-hidden-reveal", Severity::Warning,
                "Entrance markup starts hidden before progressive enhancement.".into(), "Render visible server content; start motion after a shared observer attaches. Check no-JavaScript and reduced-motion behavior."));
        }
        walk::walk_jsx_opening_element(self, node);
    }

    fn visit_jsx_element(&mut self, node: &JSXElement<'a>) {
        let korean = node.children.iter().any(inherited_korean);
        if korean {
            for item in &node.opening_element.attributes {
                if let JSXAttributeItem::Attribute(attribute) = item
                    && matches!(&attribute.name, JSXAttributeName::Identifier(name) if name.name=="wordBreak")
                    && literal(attribute, self.source).as_deref()==Some("normal") {
                    self.findings.push(site::violation(self.source, attribute.span, "korean-word-boundary", Severity::Warning,
                        "Korean content explicitly uses wordBreak normal and can split within a word.".into(), "Use keep-all with textWrap pretty so lines break on word boundaries without large trailing gaps; handle long URLs and tokens with overflowWrap break-word."));
                }
            }
        }
        walk::walk_jsx_element(self, node);
    }
}

fn inherited_korean(child: &JSXChild<'_>) -> bool {
    match child {
        JSXChild::Text(text) => text.value.chars().any(|c| ('\u{ac00}'..='\u{d7a3}').contains(&c)),
        JSXChild::Element(element) => {
            !element.opening_element.attributes.iter().any(|item| matches!(item, JSXAttributeItem::Attribute(attribute) if matches!(&attribute.name, JSXAttributeName::Identifier(name) if name.name=="wordBreak")))
                && element.children.iter().any(inherited_korean)
        }
        JSXChild::Fragment(fragment) => fragment.children.iter().any(inherited_korean),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_metadata_and_styles_are_checked_without_parsing_lookalike_strings() {
        let source = r#"const hint='// @devup-override example'; globalCss({body:{padding:'var(--space)'}}); const C=()=> <Box data-figma-node-id="1:2" bg="$secondary" _hover={{bg:'$primary'}}><Text wordBreak="normal">한글 문장</Text><Image src="/images/web-ready/abcdefabcdefabcdefabcdef.webp"/></Box>; // @devup-interaction {}"#;
        let rules = findings(source).into_iter().map(|v|v.rule).collect::<Vec<_>>();
        for rule in ["tsx-node-identifier", "variant-state-palette", "korean-word-boundary", "opaque-public-asset", "tsx-metadata-comment"] { assert!(rules.contains(&rule), "{rule}"); }
        assert_eq!(rules.iter().filter(|r|**r=="tsx-metadata-comment").count(), 1);
        assert!(!rules.contains(&"jsx-token-syntax"));
    }
    #[test]
    fn axis_suggestions_do_not_ignore_precedence_or_spreads() {
        assert!(findings(r#"const C=()=> <Box pl="2px" pr="2px"/>"#).iter().any(|v|v.rule=="axis-shorthand"));
        for source in [r#"const C=()=> <Box pl="2px" pr="3px"/>"#,r#"const C=()=> <Box {...props} pl="2px" pr="2px"/>"#,r#"const C=()=> <Box px="4px" pl="2px" pr="2px"/>"#,r#"const C=()=> <Box pl={[next()]} pr={[next()]}/>"#] {
            assert!(!findings(source).iter().any(|v|v.rule=="axis-shorthand"));
        }
    }
    #[test]
    fn explicit_child_policy_and_remote_cdn_paths_are_not_false_positives() {
        let source=r#"const C=()=> <Box wordBreak="normal"><Text wordBreak="keep-all">한글</Text><Image src="https://cdn.example.com/abcdefabcdefabcdefabcdef.webp"/></Box>"#;
        assert!(!findings(source).iter().any(|v| matches!(v.rule,"korean-word-boundary"|"opaque-public-asset")));
        assert!(findings(r#"const C=()=> <Box bg={'$secondary'} _active={{bg:'$primary'}}/>"#).iter().any(|v|v.rule=="variant-state-palette"));
    }

    #[test]
    fn source_identity_aliases_are_errors_but_example_strings_are_not_markup() {
        let source = r#"const example='<Box data-source-node-id="1:2"/>'; const C=()=> <Box data-source-node-id="1:2" data-source-node-ids="360:1:2"/>"#;
        assert_eq!(findings(source).iter().filter(|v|v.rule=="tsx-node-identifier").count(), 2);
    }
}
