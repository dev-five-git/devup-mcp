//! Site completeness: what makes a page built from Figma feel alive and ship.
//!
//! The generator emits what Figma draws, and Figma draws a resting state. A
//! site built from it alone had no hover, focus or pressed feedback, no
//! transitions and no metadata, and nothing said so. These checks name each
//! gap in the code a caller supplies; they open no file.
//!
//! - `interactive-states` (warning): a link, button or click target with
//!   neither `_hover` nor `_focusVisible`. Without a visible focus state a
//!   keyboard user cannot see where they are.
//! - `hover-transition` (info): a `_hover` that changes something with no
//!   `transition`, so the change snaps instead of easing.
//! - `page-metadata` (warning): an App Router `page` or `layout` that exports
//!   neither `metadata` nor `generateMetadata`.

use super::{Severity, Violation};
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};

/// Interactive elements found in one source, and how many carry states.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InteractionCoverage {
    pub interactive: usize,
    pub with_states: usize,
    pub hover_without_transition: usize,
}

struct Element {
    span: Span,
    name: String,
    has_states: bool,
    hover_without_transition: bool,
}

#[derive(Default)]
struct Collector {
    elements: Vec<Element>,
}

fn attribute_name<'a>(item: &'a JSXAttributeItem<'a>) -> Option<&'a str> {
    match item {
        JSXAttributeItem::Attribute(attribute) => match &attribute.name {
            JSXAttributeName::Identifier(name) => Some(name.name.as_str()),
            JSXAttributeName::NamespacedName(_) => None,
        },
        JSXAttributeItem::SpreadAttribute(_) => None,
    }
}

/// `as="a"`, `as="button"` or `as={Link}`.
fn interactive_as(item: &JSXAttributeItem<'_>) -> bool {
    let JSXAttributeItem::Attribute(attribute) = item else {
        return false;
    };
    if !matches!(&attribute.name, JSXAttributeName::Identifier(name) if name.name == "as") {
        return false;
    }
    match &attribute.value {
        Some(JSXAttributeValue::StringLiteral(value)) => {
            matches!(value.value.as_str(), "a" | "button")
        }
        Some(JSXAttributeValue::ExpressionContainer(container)) => {
            matches!(&container.expression, JSXExpression::Identifier(id) if id.name == "Link")
        }
        _ => false,
    }
}

impl<'a> Visit<'a> for Collector {
    fn visit_jsx_opening_element(&mut self, node: &JSXOpeningElement<'a>) {
        let name = match &node.name {
            JSXElementName::Identifier(id) => id.name.to_string(),
            JSXElementName::IdentifierReference(id) => id.name.to_string(),
            _ => String::new(),
        };
        let names = node
            .attributes
            .iter()
            .filter_map(attribute_name)
            .collect::<Vec<_>>();
        // A spread may carry the states; nothing here can see into it.
        let spread = node
            .attributes
            .iter()
            .any(|item| matches!(item, JSXAttributeItem::SpreadAttribute(_)));
        let interactive = matches!(name.as_str(), "a" | "button" | "Link")
            || node.attributes.iter().any(interactive_as)
            || names.contains(&"onClick");
        if interactive && !spread {
            let hover = names.contains(&"_hover");
            let transition = names.iter().any(|name| name.starts_with("transition"));
            self.elements.push(Element {
                span: node.span(),
                name,
                has_states: hover || names.contains(&"_focusVisible"),
                hover_without_transition: hover && !transition,
            });
        }
        walk::walk_jsx_opening_element(self, node);
    }
}

fn source_type(path: &str) -> SourceType {
    if path.ends_with(".ts") {
        SourceType::ts()
    } else {
        SourceType::tsx()
    }
}

fn collect(path: &str, source: &str) -> Option<Vec<Element>> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type(path)).parse();
    if parsed.panicked || !parsed.diagnostics.is_empty() {
        return None;
    }
    let mut collector = Collector::default();
    collector.visit_program(&parsed.program);
    Some(collector.elements)
}

/// How many interactive elements one source has, and how many carry a hover
/// or focus state. A source that does not parse counts as none.
pub fn interaction_coverage(path: &str, source: &str) -> InteractionCoverage {
    let elements = collect(path, source).unwrap_or_default();
    InteractionCoverage {
        interactive: elements.len(),
        with_states: elements.iter().filter(|element| element.has_states).count(),
        hover_without_transition: elements
            .iter()
            .filter(|element| element.hover_without_transition)
            .count(),
    }
}

fn violation(
    source: &str,
    span: Span,
    rule: &'static str,
    severity: Severity,
    message: String,
    suggestion: &str,
) -> Violation {
    let start = span.start as usize;
    let prefix = source.get(..start).unwrap_or("");
    let mut context = serde_json::Map::new();
    context.insert(
        "line".into(),
        serde_json::json!(prefix.bytes().filter(|b| *b == b'\n').count() + 1),
    );
    // The same position fields every structural finding carries.
    context.insert(
        "column".into(),
        serde_json::json!(prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1),
    );
    context.insert(
        "positionEncoding".into(),
        serde_json::json!("1-based Unicode scalar columns; byteRange uses UTF-8 bytes"),
    );
    context.insert(
        "snippet".into(),
        serde_json::json!(
            source
                .get(start..span.end as usize)
                .unwrap_or("")
                .chars()
                .take(120)
                .collect::<String>()
        ),
    );
    Violation {
        rule,
        severity,
        byte_range: [start, span.end as usize],
        context,
        message,
        suggestion: Some(suggestion.into()),
    }
}

/// `interactive-states` and `hover-transition` findings for one source.
pub fn interaction_findings(path: &str, source: &str) -> Vec<Violation> {
    let mut found = Vec::new();
    for element in collect(path, source).unwrap_or_default() {
        if !element.has_states {
            found.push(violation(
                source,
                element.span,
                "interactive-states",
                Severity::Warning,
                format!(
                    "<{}> is interactive but has neither _hover nor _focusVisible; it gives no feedback and a keyboard user cannot see focus on it.",
                    element.name
                ),
                "Add _hover, _focusVisible (a visible focus ring), _active, cursor=\"pointer\" and _disabled where it applies, built from existing theme tokens, with a shared transition. On a next/link, render it as <Box as={Link}> so the states are devup-ui props.",
            ));
        } else if element.hover_without_transition {
            found.push(violation(
                source,
                element.span,
                "hover-transition",
                Severity::Info,
                format!(
                    "<{}> changes on _hover with no transition, so the change snaps.",
                    element.name
                ),
                "Add one shared transition (about 0.2s ease on the properties that change) and _motionReduce={{ transition: \"none\" }}.",
            ));
        }
    }
    found
}

/// Whether a path is an App Router `page` or `layout` file.
pub fn is_route_file(path: &str) -> bool {
    let path = path.replace('\\', "/");
    let mut segments = path.split('/').collect::<Vec<_>>();
    let Some(file) = segments.pop() else {
        return false;
    };
    let stem = file.split('.').next().unwrap_or_default();
    matches!(stem, "page" | "layout")
        && file
            .rsplit('.')
            .next()
            .is_some_and(|ext| matches!(ext, "tsx" | "ts" | "jsx" | "js"))
        && segments.contains(&"app")
}

/// Whether a source exports `metadata` or `generateMetadata`.
pub fn exports_metadata(source: &str) -> bool {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    let named = |name: &str| matches!(name, "metadata" | "generateMetadata");
    parsed.program.body.iter().any(|statement| match statement {
        Statement::ExportDeclaration(export) => match &export.declaration {
            Declaration::VariableDeclaration(variables) => {
                variables.declarations.iter().any(|declarator| {
                    matches!(&declarator.id, BindingPattern::BindingIdentifier(id) if named(&id.name))
                })
            }
            Declaration::FunctionDeclaration(function) => function
                .id
                .as_ref()
                .is_some_and(|id| id.name == "generateMetadata"),
            _ => false,
        },
        Statement::ExportNamedDeclaration(export) => export
            .specifiers
            .iter()
            .any(|specifier| named(&specifier.exported.to_string())),
        _ => false,
    })
}

/// `page-metadata` for one file, when it is a page or layout without one.
pub fn metadata_finding(path: &str, source: &str) -> Option<Violation> {
    if !is_route_file(path) || exports_metadata(source) {
        return None;
    }
    Some(violation(
        source,
        Span::new(0, 0),
        "page-metadata",
        Severity::Warning,
        format!(
            "{path} exports no metadata, so the page has no title, description or share card of its own."
        ),
        "Export metadata with title, description and openGraph (the devfive-frontend SEO rule); the root layout also carries the title template and metadataBase. A 'use client' page cannot export it: move the client part into a component.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_link_or_button_is_a_warning_and_a_styled_one_is_not() {
        let source = r#"import Link from "next/link";
export function Nav() {
  return <>
    <Link href="/about">About</Link>
    <Box as="button" onClick={go}>Go</Box>
    <Box as={Link} href="/x" _hover={{ color: "$primary" }} _focusVisible={{ outline: "2px solid $primary" }} transition="all 0.2s ease">X</Box>
    <Box as="a" href="/y" _hover={{ color: "$primary" }}>Y</Box>
    <Box {...props} onClick={go} />
  </>;
}"#;
        let found = interaction_findings("Nav.tsx", source);
        let rules = found.iter().map(|v| v.rule).collect::<Vec<_>>();
        assert_eq!(
            rules,
            [
                "interactive-states",
                "interactive-states",
                "hover-transition"
            ]
        );
        assert_eq!(found[0].severity, Severity::Warning);
        assert_eq!(found[2].severity, Severity::Info);
        let coverage = interaction_coverage("Nav.tsx", source);
        assert_eq!(
            coverage,
            InteractionCoverage {
                interactive: 4,
                with_states: 2,
                hover_without_transition: 1
            }
        );
    }

    #[test]
    fn a_page_or_layout_without_metadata_is_a_warning() {
        let bare = "export default function Page() { return <main />; }";
        let finding = metadata_finding("src/app/about/page.tsx", bare).unwrap();
        assert_eq!(finding.rule, "page-metadata");
        assert_eq!(finding.severity, Severity::Warning);
        for with in [
            "export const metadata = { title: 'About' };\nexport default function Page() { return <main />; }",
            "export async function generateMetadata() { return {}; }\nexport default function Page() { return <main />; }",
        ] {
            assert!(metadata_finding("app/about/page.tsx", with).is_none());
        }
        assert!(metadata_finding("src/components/About.tsx", bare).is_none());
        assert!(is_route_file("app/layout.tsx"));
    }
}
