//! Site completeness: what makes a page built from Figma feel alive and ship.
//!
//! The generator emits what Figma draws, and Figma draws a resting state. A
//! site built from it alone had no hover, focus or pressed feedback, no
//! transitions and no metadata, and nothing said so. These checks name each
//! gap in the code a caller supplies; they open no file.
//!
//! Interactive elements are sorted into roles - button, icon button, link,
//! nav item, card, input, disclosure - because each role owes different
//! feedback (the devup-implementation-plan skill's recipe table), and because
//! "too much" is as real a failure as "nothing":
//!
//! - `interactive-states` (warning): neither a hover nor a focus style.
//! - `focus-style` (warning): a hover but no focus style. Hover alone leaves
//!   a keyboard user unable to see where they are.
//! - `button-active` (info): a button with a hover but no pressed state.
//! - `hover-transition` (info): a `_hover` with no `transition`, so it snaps.
//! - `too-much-motion` (warning): a transition or animation over 400ms, a
//!   scale above 1.05, or an `infinite` animation.
//! - `motion-reduce` (info): a transform or animation with no `_motionReduce`.
//! - `icon-button-label` (warning): a button with no text and no `aria-label`.
//! - `nav-current` (info): a link in a nav with no `aria-current` handling.
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

/// What an interactive element is, which decides the feedback it owes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Button,
    IconButton,
    Link,
    NavItem,
    Card,
    Input,
    Disclosure,
    Clickable,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Button => "buttons",
            Self::IconButton => "icon buttons",
            Self::Link => "links",
            Self::NavItem => "nav items",
            Self::Card => "cards",
            Self::Input => "inputs",
            Self::Disclosure => "disclosures",
            Self::Clickable => "click targets",
        }
    }

    /// The states a complete element of this role carries, as status names
    /// them.
    pub fn requirement(self) -> &'static str {
        match self {
            Self::Button => "hover+active+focus",
            Self::IconButton => "hover+focus+aria-label",
            Self::NavItem => "hover+focus+aria-current",
            Self::Input => "hover+focus",
            Self::Disclosure => "focus+aria-expanded",
            Self::Link | Self::Card | Self::Clickable => "hover+focus",
        }
    }
}

/// How many elements of one role a source has, and how many are complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoleCoverage {
    pub role: Role,
    pub total: usize,
    pub complete: usize,
}

struct Element {
    span: Span,
    name: String,
    role: Role,
    hover: bool,
    focus: bool,
    active: bool,
    transition: bool,
    motion_reduce: bool,
    aria_label: bool,
    aria_current: bool,
    /// The opening tag as written, for the motion checks.
    text: String,
}

impl Element {
    fn complete(&self) -> bool {
        match self.role {
            Role::Button => self.hover && self.active && self.focus,
            Role::IconButton => self.hover && self.focus && self.aria_label,
            Role::NavItem => self.hover && self.focus && self.aria_current,
            Role::Disclosure => self.focus,
            Role::Input | Role::Link | Role::Card | Role::Clickable => self.hover && self.focus,
        }
    }
}

struct Collector<'s> {
    source: &'s str,
    nav_depth: usize,
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

/// The literal or identifier `as` names, if any: `as="a"`, `as={Link}`.
fn as_value(node: &JSXOpeningElement<'_>) -> Option<String> {
    node.attributes.iter().find_map(|item| {
        let JSXAttributeItem::Attribute(attribute) = item else {
            return None;
        };
        if !matches!(&attribute.name, JSXAttributeName::Identifier(name) if name.name == "as") {
            return None;
        }
        match &attribute.value {
            Some(JSXAttributeValue::StringLiteral(value)) => Some(value.value.to_string()),
            Some(JSXAttributeValue::ExpressionContainer(container)) => {
                match &container.expression {
                    JSXExpression::Identifier(id) => Some(id.name.to_string()),
                    _ => None,
                }
            }
            _ => None,
        }
    })
}

fn element_name(node: &JSXOpeningElement<'_>) -> String {
    match &node.name {
        JSXElementName::Identifier(id) => id.name.to_string(),
        JSXElementName::IdentifierReference(id) => id.name.to_string(),
        _ => String::new(),
    }
}

/// Whether any descendant is an image, which makes a link a card.
fn has_image(children: &[JSXChild<'_>]) -> bool {
    children.iter().any(|child| match child {
        JSXChild::Element(element) => {
            matches!(
                element_name(&element.opening_element).as_str(),
                "Image" | "img"
            ) || has_image(&element.children)
        }
        JSXChild::Fragment(fragment) => has_image(&fragment.children),
        _ => false,
    })
}

/// Whether the element shows text of its own: literal text, or an
/// expression that may render text. An icon button shows neither.
fn has_text(children: &[JSXChild<'_>]) -> bool {
    children.iter().any(|child| match child {
        JSXChild::Text(text) => !text.value.trim().is_empty(),
        JSXChild::ExpressionContainer(container) => {
            !matches!(container.expression, JSXExpression::EmptyExpression(_))
        }
        JSXChild::Element(element) => {
            !matches!(
                element_name(&element.opening_element).as_str(),
                "Image" | "img" | "svg"
            ) && has_text(&element.children)
        }
        JSXChild::Fragment(fragment) => has_text(&fragment.children),
        JSXChild::Spread(_) => true,
    })
}

impl<'a> Visit<'a> for Collector<'_> {
    fn visit_jsx_element(&mut self, node: &JSXElement<'a>) {
        let opening = &node.opening_element;
        let tag = element_name(opening);
        let as_tag = as_value(opening);
        let names = opening
            .attributes
            .iter()
            .filter_map(attribute_name)
            .collect::<Vec<_>>();
        let has = |name: &str| names.contains(&name);
        let effective = as_tag.clone().unwrap_or_else(|| tag.clone());
        let nav = effective == "nav"
            || opening.attributes.iter().any(|item| {
                matches!(item, JSXAttributeItem::Attribute(attribute)
                    if matches!(&attribute.name, JSXAttributeName::Identifier(name) if name.name == "role")
                    && matches!(&attribute.value, Some(JSXAttributeValue::StringLiteral(value)) if value.value == "navigation"))
            });
        // A spread may carry the states; nothing here can see into it.
        let spread = opening
            .attributes
            .iter()
            .any(|item| matches!(item, JSXAttributeItem::SpreadAttribute(_)));
        let link = matches!(effective.as_str(), "a" | "Link");
        let button = effective == "button";
        let input = matches!(effective.as_str(), "input" | "select" | "textarea");
        let role = if input {
            Some(Role::Input)
        } else if has("aria-expanded") {
            Some(Role::Disclosure)
        } else if button {
            Some(if has_text(&node.children) {
                Role::Button
            } else {
                Role::IconButton
            })
        } else if link {
            Some(if self.nav_depth > 0 || has("aria-current") {
                Role::NavItem
            } else if has_image(&node.children) {
                Role::Card
            } else {
                Role::Link
            })
        } else if has("onClick") {
            Some(Role::Clickable)
        } else {
            None
        };
        if let Some(role) = role.filter(|_| !spread) {
            let span = opening.span();
            self.elements.push(Element {
                span,
                name: tag,
                role,
                hover: has("_hover"),
                focus: has("_focusVisible") || has("_focus") || has("_focusWithin"),
                active: has("_active"),
                transition: names.iter().any(|name| name.starts_with("transition")),
                motion_reduce: has("_motionReduce"),
                aria_label: has("aria-label") || has("aria-labelledby"),
                aria_current: has("aria-current"),
                text: self
                    .source
                    .get(span.start as usize..span.end as usize)
                    .unwrap_or_default()
                    .to_owned(),
            });
        }
        self.nav_depth += usize::from(nav);
        walk::walk_jsx_element(self, node);
        self.nav_depth -= usize::from(nav);
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
    let mut collector = Collector {
        source,
        nav_depth: 0,
        elements: Vec::new(),
    };
    collector.visit_program(&parsed.program);
    Some(collector.elements)
}

/// How many interactive elements one source has, and how many carry a hover
/// or focus state. A source that does not parse counts as none.
pub fn interaction_coverage(path: &str, source: &str) -> InteractionCoverage {
    let elements = collect(path, source).unwrap_or_default();
    InteractionCoverage {
        interactive: elements.len(),
        with_states: elements
            .iter()
            .filter(|element| element.hover || element.focus)
            .count(),
        hover_without_transition: elements
            .iter()
            .filter(|element| element.hover && !element.transition)
            .count(),
    }
}

/// Per role, how many elements a source has and how many carry everything
/// that role owes. Roles with no element are left out.
pub fn role_coverage(path: &str, source: &str) -> Vec<RoleCoverage> {
    let elements = collect(path, source).unwrap_or_default();
    let mut roles = elements
        .iter()
        .map(|element| element.role)
        .collect::<Vec<_>>();
    roles.sort();
    roles.dedup();
    roles
        .into_iter()
        .map(|role| RoleCoverage {
            role,
            total: elements
                .iter()
                .filter(|element| element.role == role)
                .count(),
            complete: elements
                .iter()
                .filter(|element| element.role == role && element.complete())
                .count(),
        })
        .collect()
}

/// Milliseconds of every duration written in a tag: `0.2s`, `250ms`, and a
/// bare number on a `transitionDuration`/`animationDuration` prop, which
/// devup-ui reads as milliseconds.
fn durations(text: &str) -> Vec<f64> {
    let mut found = Vec::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_digit()
            && (index == 0
                || !(bytes[index - 1].is_ascii_alphanumeric() || bytes[index - 1] == b'.'))
        {
            let start = index;
            while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'.') {
                index += 1;
            }
            let number = text[start..index].parse::<f64>().unwrap_or(0.0);
            let rest = &text[index..];
            let unit_end = |unit: &str| {
                rest.starts_with(unit)
                    && !rest[unit.len()..]
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphanumeric())
            };
            if unit_end("ms") {
                found.push(number);
            } else if unit_end("s") {
                found.push(number * 1000.0);
            }
            continue;
        }
        index += 1;
    }
    for prop in ["transitionDuration={", "animationDuration={"] {
        if let Some(at) = text.find(prop) {
            let value = &text[at + prop.len()..];
            let end = value.find('}').unwrap_or(0);
            if let Ok(number) = value[..end].trim().parse::<f64>() {
                found.push(number);
            }
        }
    }
    found
}

/// Every `scale(...)` factor in a tag.
fn scales(text: &str) -> Vec<f64> {
    text.match_indices("scale(")
        .filter_map(|(at, _)| {
            let value = &text[at + "scale(".len()..];
            let end = value.find([')', ','])?;
            value[..end].trim().parse::<f64>().ok()
        })
        .collect()
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

/// Every interaction finding for one source: missing feedback, and too
/// much of it.
pub fn interaction_findings(path: &str, source: &str) -> Vec<Violation> {
    let mut found = Vec::new();
    for element in collect(path, source).unwrap_or_default() {
        let name = &element.name;
        let mut push =
            |rule: &'static str, severity: Severity, message: String, suggestion: &str| {
                found.push(violation(
                    source,
                    element.span,
                    rule,
                    severity,
                    message,
                    suggestion,
                ));
            };
        if !element.hover && !element.focus {
            push(
                "interactive-states",
                Severity::Warning,
                format!(
                    "<{name}> is interactive ({}) but has neither a hover nor a focus style; it gives no feedback and a keyboard user cannot see focus on it.",
                    element.role.as_str()
                ),
                "Apply the role's recipe from the devup-implementation-plan skill: hover, _focusVisible (2px ring with offset), _active, cursor=\"pointer\" and _disabled where it applies, from existing theme tokens, with the shared transition. On a next/link, render it as <Box as={Link}> so the states are devup-ui props.",
            );
        } else if !element.focus {
            push(
                "focus-style",
                Severity::Warning,
                format!(
                    "<{name}> has a hover but no focus style; a keyboard user cannot see focus on it."
                ),
                "Add _focusVisible with a visible ring (2px with offset) from an existing token. Hover alone is not enough.",
            );
        }
        if matches!(element.role, Role::Button) && element.hover && !element.active {
            push(
                "button-active",
                Severity::Info,
                format!("<{name}> responds to hover but not to being pressed."),
                "Add _active: a pressed scale(0.98) or one step darker than the hover token.",
            );
        }
        if element.hover && !element.transition {
            push(
                "hover-transition",
                Severity::Info,
                format!("<{name}> changes on _hover with no transition, so the change snaps."),
                "Use the shared transition (150-250ms, ease or ease-out, defined once) on the properties that change.",
            );
        }
        let slow = durations(&element.text).into_iter().find(|ms| *ms > 400.0);
        let big = scales(&element.text)
            .into_iter()
            .find(|scale| *scale > 1.05);
        let infinite = element.text.contains("infinite");
        if slow.is_some() || big.is_some() || infinite {
            let mut why = Vec::new();
            if let Some(ms) = slow {
                why.push(format!("a {ms}ms duration"));
            }
            if let Some(scale) = big {
                why.push(format!("scale({scale})"));
            }
            if infinite {
                why.push("an infinite animation".into());
            }
            push(
                "too-much-motion",
                Severity::Warning,
                format!("<{name}> has too much motion: {}.", why.join(", ")),
                "Keep durations at 150-250ms, scale between 0.97 and 1.05, movement at 4px or less, and no infinite or attention-seeking animation.",
            );
        }
        let moves = ["transform", "translate", "scale(", "rotate", "animation"]
            .iter()
            .any(|word| element.text.contains(word));
        if moves && !element.motion_reduce {
            push(
                "motion-reduce",
                Severity::Info,
                format!("<{name}> moves, and nothing turns that off for prefers-reduced-motion."),
                "Add _motionReduce={{ transform: \"none\", transition: \"none\" }}, or one globalCss _motionReduce rule for the site.",
            );
        }
        if matches!(element.role, Role::IconButton) && !element.aria_label {
            push(
                "icon-button-label",
                Severity::Warning,
                format!(
                    "<{name}> shows no text and has no aria-label, so a screen reader announces nothing."
                ),
                "Add aria-label naming what the button does.",
            );
        }
        if matches!(element.role, Role::NavItem) && !element.aria_current {
            push(
                "nav-current",
                Severity::Info,
                format!("<{name}> is in a nav but never marks the current page."),
                "Set aria-current=\"page\" on the link for the current route (usePathname) and give it the Figma active style.",
            );
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
                "focus-style",
                "hover-transition"
            ]
        );
        assert_eq!(found[0].severity, Severity::Warning);
        assert_eq!(found[2].severity, Severity::Warning);
        assert_eq!(found[3].severity, Severity::Info);
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

    fn rules_of(source: &str) -> Vec<&'static str> {
        interaction_findings("Page.tsx", source)
            .iter()
            .map(|finding| finding.rule)
            .collect()
    }

    #[test]
    fn a_button_owes_a_pressed_state_and_an_icon_button_a_label() {
        let rules = rules_of(
            r#"export function Cta() { return <>
  <Box as="button" _hover={{ bg: "$primaryBold" }} _focusVisible={{ outline: "2px solid $primary" }} transition="all 0.2s ease">Send</Box>
  <Box as="button" _hover={{ bg: "$gray100" }} _active={{ bg: "$gray200" }} _focusVisible={{ outline: "2px solid $primary" }} transition="all 0.2s ease"><Image src="/icons/close.svg" alt="" /></Box>
</>; }"#,
        );
        assert_eq!(rules, ["button-active", "icon-button-label"]);
    }

    #[test]
    fn too_much_motion_and_unguarded_motion_are_reported() {
        let rules = rules_of(
            r#"export function Hero() { return <>
  <Box as="a" href="/x" _hover={{ transform: "scale(1.1)" }} _focusVisible={{ outline: "2px solid $primary" }} transition="all 0.6s ease" _motionReduce={{ transform: "none" }}>X</Box>
  <Box as="a" href="/y" animation="pulse 1s infinite" _hover={{ color: "$primary" }} _focusVisible={{ outline: "2px solid $primary" }} transition="color 0.2s ease">Y</Box>
  <Box as="a" href="/z" _hover={{ transform: "translateY(-2px)" }} _focusVisible={{ outline: "2px solid $primary" }} transition="transform 200ms ease-out">Z</Box>
</>; }"#,
        );
        assert_eq!(
            rules,
            [
                "too-much-motion",
                "too-much-motion",
                "motion-reduce",
                "motion-reduce"
            ]
        );
        assert_eq!(
            durations(r#"transition="all 0.25s, color 150ms" transitionDuration={500}"#),
            [250.0, 150.0, 500.0]
        );
        assert_eq!(scales("scale(1.04) scale(0.98)"), [1.04, 0.98]);
    }

    #[test]
    fn nav_links_should_mark_the_current_page_and_roles_are_counted() {
        let source = r#"export function Header() { return <nav>
  <Box as={Link} href="/" _hover={{ color: "$primary" }} _focusVisible={{ outline: "2px solid $primary" }} transition="color 0.2s ease">Home</Box>
  <Box as={Link} href="/about" aria-current="page" _hover={{ color: "$primary" }} _focusVisible={{ outline: "2px solid $primary" }} transition="color 0.2s ease">About</Box>
</nav>; }
export function Cards() { return <Box as={Link} href="/work/1"><Image src="/a.png" alt="A" /></Box>; }"#;
        assert_eq!(rules_of(source), ["nav-current", "interactive-states"]);
        let coverage = role_coverage("Header.tsx", source)
            .into_iter()
            .map(|row| (row.role.as_str(), row.total, row.complete))
            .collect::<Vec<_>>();
        assert_eq!(coverage, [("nav items", 2, 1), ("cards", 1, 0)]);
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
