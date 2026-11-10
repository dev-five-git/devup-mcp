//! Entrance-animation mistakes that the source alone proves.
//!
//! Only shapes with one reading are reported. Whether motion suits a section,
//! how long it runs and when it fires in a real browser are judged by the
//! runtime audit and the delivery evidence, not here.
//!
//! - `ssr-hidden-reveal`: a `motion.*` element that is always rendered starts
//!   at `opacity: 0`. Server markup carries that style inline, so the content
//!   is invisible until JavaScript hydrates and animates it — and stays
//!   invisible when JavaScript fails or is slow.
//! - `reveal-replays`: `whileInView` without `viewport={{ once: true }}`
//!   plays again on every re-entry.
//! - `reveal-moves-background`: the element that carries the section's paint
//!   is the one being faded or moved, so the background animates with it.
//! - `motion-without-reduced-motion`: entrance motion with no reduced-motion
//!   handling anywhere in the validated sources (cross-file, see
//!   [`reduced_motion_finding`]).
use super::{Severity, Violation, site};
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::SourceType;

const BACKGROUND_PROPS: &[&str] =
    &["bg", "bgColor", "bgImage", "background", "backgroundColor", "backgroundImage"];

pub(super) fn findings(source: &str) -> Vec<Violation> {
    collect(source).map(|collector| collector.findings).unwrap_or_default()
}

/// One finding for a file that animates content into view while nothing in it
/// handles reduced motion. Callers that can see other files drop it when any
/// of them [`handles_reduced_motion`].
pub fn reduced_motion_finding(source: &str) -> Option<Violation> {
    if handles_reduced_motion(source) {
        return None;
    }
    let span = collect(source)?.first_entrance?;
    Some(site::violation(
        source,
        span,
        "motion-without-reduced-motion",
        Severity::Warning,
        "Entrance motion is added, and no validated source handles prefers-reduced-motion.".into(),
        "Leave content in place for reduced motion: useReducedMotion(), <MotionConfig reducedMotion=\"user\"> in a shared provider, or a prefers-reduced-motion media rule. Validate the provider together with this file if it lives elsewhere.",
    ))
}

/// Whether this source reads the user's motion preference or configures it
/// for the tree (`useReducedMotion`, `MotionConfig reducedMotion`, a
/// `prefers-reduced-motion` media rule or query).
pub fn handles_reduced_motion(source: &str) -> bool {
    ["educedMotion", "prefers-reduced-motion", "_motionReduce"]
        .iter()
        .any(|needle| source.contains(needle))
}

fn collect(source: &str) -> Option<Collector<'_>> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    if parsed.panicked || !parsed.diagnostics.is_empty() {
        return None;
    }
    let mut collector =
        Collector { source, findings: Vec::new(), first_entrance: None, optional: 0 };
    collector.visit_program(&parsed.program);
    Some(collector)
}

struct Collector<'s> {
    source: &'s str,
    findings: Vec<Violation>,
    first_entrance: Option<oxc_span::Span>,
    /// Depth inside `AnimatePresence` or a conditional render: such an
    /// element mounts on the client later, so its initial style is not
    /// server markup.
    optional: usize,
}

fn attribute<'b, 'a>(node: &'b JSXOpeningElement<'a>, name: &str) -> Option<&'b JSXAttribute<'a>> {
    node.attributes.iter().find_map(|item| match item {
        JSXAttributeItem::Attribute(attribute)
            if matches!(&attribute.name, JSXAttributeName::Identifier(id) if id.name == name) =>
        {
            Some(&**attribute)
        }
        _ => None,
    })
}

fn expression<'b, 'a>(attribute: &'b JSXAttribute<'a>) -> Option<&'b Expression<'a>> {
    match attribute.value.as_ref()? {
        JSXAttributeValue::ExpressionContainer(container) => {
            container.expression.as_expression().map(Expression::get_inner_expression)
        }
        _ => None,
    }
}

fn string<'b>(attribute: &'b JSXAttribute<'_>) -> Option<&'b str> {
    match attribute.value.as_ref()? {
        JSXAttributeValue::StringLiteral(value) => Some(value.value.as_str()),
        _ => match expression(attribute)? {
            Expression::StringLiteral(value) => Some(value.value.as_str()),
            _ => None,
        },
    }
}

fn property<'b, 'a>(object: &'b ObjectExpression<'a>, name: &str) -> Option<&'b Expression<'a>> {
    object.properties.iter().find_map(|item| match item {
        ObjectPropertyKind::ObjectProperty(property)
            if property.key.static_name().is_some_and(|key| key == name) =>
        {
            Some(property.value.get_inner_expression())
        }
        _ => None,
    })
}

fn is_zero(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::NumericLiteral(number) => number.value == 0.0,
        Expression::StringLiteral(text) => text.value.trim() == "0",
        _ => false,
    }
}

fn hides(expression: &Expression<'_>) -> bool {
    matches!(expression, Expression::ObjectExpression(object) if property(object, "opacity").is_some_and(is_zero))
}

fn is_motion_element(node: &JSXOpeningElement<'_>) -> bool {
    matches!(&node.name, JSXElementName::MemberExpression(member)
        if matches!(&member.object, JSXMemberExpressionObject::IdentifierReference(object) if matches!(object.name.as_str(), "motion" | "m")))
}

/// `initial={{ opacity: 0 }}`, or `initial="hidden"` naming a variant this
/// same element declares with `opacity: 0`. A variant declared elsewhere is
/// not followed: it cannot be proven from here.
fn starts_hidden(node: &JSXOpeningElement<'_>) -> bool {
    let Some(initial) = attribute(node, "initial") else {
        return false;
    };
    if expression(initial).is_some_and(hides) {
        return true;
    }
    let Some(name) = string(initial) else {
        return false;
    };
    attribute(node, "variants")
        .and_then(expression)
        .and_then(|variants| match variants {
            Expression::ObjectExpression(object) => property(object, name),
            _ => None,
        })
        .is_some_and(hides)
}

fn paints_background(node: &JSXOpeningElement<'_>) -> bool {
    if BACKGROUND_PROPS.iter().any(|name| attribute(node, name).is_some()) {
        return true;
    }
    // `className={css({ bg: … })}` on a motion element.
    attribute(node, "className").and_then(expression).is_some_and(|value| match value {
        Expression::CallExpression(call) => call.arguments.first().is_some_and(|argument| {
            matches!(argument, Argument::ObjectExpression(object)
                if BACKGROUND_PROPS.iter().any(|name| property(object, name).is_some()))
        }),
        _ => false,
    })
}

impl<'a> Visit<'a> for Collector<'_> {
    fn visit_jsx_element(&mut self, node: &JSXElement<'a>) {
        let presence = node
            .opening_element
            .name
            .get_identifier_name()
            .is_some_and(|name| name == "AnimatePresence");
        self.optional += usize::from(presence);
        walk::walk_jsx_element(self, node);
        self.optional -= usize::from(presence);
    }

    fn visit_logical_expression(&mut self, node: &LogicalExpression<'a>) {
        self.optional += 1;
        walk::walk_logical_expression(self, node);
        self.optional -= 1;
    }

    fn visit_conditional_expression(&mut self, node: &ConditionalExpression<'a>) {
        self.optional += 1;
        walk::walk_conditional_expression(self, node);
        self.optional -= 1;
    }

    fn visit_jsx_opening_element(&mut self, node: &JSXOpeningElement<'a>) {
        let in_view = attribute(node, "whileInView");
        let hidden = is_motion_element(node) && starts_hidden(node);
        if (in_view.is_some() || hidden) && self.first_entrance.is_none() {
            self.first_entrance = Some(node.span);
        }
        if hidden && self.optional == 0 {
            self.findings.push(site::violation(self.source, node.span, "ssr-hidden-reveal", Severity::Warning,
                "Server markup renders this content at opacity 0 until JavaScript animates it.".into(),
                "Render visible server content and let a shared observer start the motion after it attaches, or make the hidden start conditional on a mounted/enhanced state. Check no-JavaScript, slow hydration, reload and client navigation."));
        }
        if let Some(trigger) = in_view {
            let once = match attribute(node, "viewport") {
                None => Some(false),
                Some(viewport) => match expression(viewport) {
                    Some(Expression::ObjectExpression(object)) => Some(
                        matches!(property(object, "once"), Some(Expression::BooleanLiteral(once)) if once.value),
                    ),
                    // A viewport built elsewhere cannot be read from here.
                    _ => None,
                },
            };
            if once == Some(false) {
                self.findings.push(site::violation(self.source, trigger.span, "reveal-replays", Severity::Warning,
                    "This scroll reveal has no one-shot viewport, so it replays on every re-entry.".into(),
                    "Add viewport={{ once: true }} (with an amount/margin that fires when the content is actually visible). Keep a replay only when the design explicitly asks for it."));
            }
            if paints_background(node) {
                self.findings.push(site::violation(self.source, trigger.span, "reveal-moves-background", Severity::Warning,
                    "The element that paints the section background is the one being revealed, so the background fades or moves with it.".into(),
                    "Keep the painted section fixed and reveal an inner content wrapper."));
            }
        }
        walk::walk_jsx_opening_element(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(source: &str) -> Vec<&'static str> {
        findings(source).into_iter().map(|finding| finding.rule).collect()
    }

    #[test]
    fn always_rendered_motion_that_starts_at_opacity_zero_is_hidden_server_markup() {
        for source in [
            r#"const C = () => <motion.div initial={{ opacity: 0, y: 24 }} animate={{ opacity: 1, y: 0 }}>Copy</motion.div>"#,
            r#"const C = () => <motion.section initial="hidden" variants={{ hidden: { opacity: 0 }, visible: { opacity: 1 } }} whileInView="visible" viewport={{ once: true }}>Copy</motion.section>"#,
            r#"const C = () => <ul>{items.map(item => <m.li key={item.id} initial={{ opacity: '0' }} animate={{ opacity: 1 }}>{item.name}</m.li>)}</ul>"#,
        ] {
            assert_eq!(rules(source), ["ssr-hidden-reveal"], "{source}");
        }
    }

    #[test]
    fn client_mounted_and_unprovable_starts_are_not_reported() {
        for source in [
            // Mounted later on the client: never part of server markup.
            r#"const C = () => <AnimatePresence>{open && <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} />}</AnimatePresence>"#,
            r#"const C = () => open ? <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} /> : null"#,
            r#"const C = () => <AnimatePresence><motion.div key={step} initial={{ opacity: 0 }} animate={{ opacity: 1 }} /></AnimatePresence>"#,
            // Visible start, disabled initial, or a start decided at runtime.
            r#"const C = () => <motion.div initial={{ opacity: 1, y: 24 }} animate={{ y: 0 }} />"#,
            r#"const C = () => <motion.div initial={false} animate={{ opacity: 1 }} />"#,
            r#"const C = () => <motion.div initial={enhanced ? { opacity: 0 } : false} animate={{ opacity: 1 }} />"#,
            // The variant lives in another element or module.
            r#"const C = () => <motion.div initial="hidden" variants={variants} animate="visible" />"#,
            // Not a motion element: `initial` means something else here.
            r#"const C = () => <Form initial={{ opacity: 0 }} />"#,
        ] {
            assert_eq!(rules(source), Vec::<&str>::new(), "{source}");
        }
    }

    #[test]
    fn a_scroll_reveal_needs_one_shot_semantics() {
        for source in [
            r#"const C = () => <motion.div whileInView={{ y: 0 }} />"#,
            r#"const C = () => <motion.div whileInView="visible" viewport={{ amount: 0.2 }} />"#,
            r#"const C = () => <motion.div whileInView="visible" viewport={{ once: false }} />"#,
            r#"const C = () => <Reveal whileInView="visible" />"#,
        ] {
            assert_eq!(rules(source), ["reveal-replays"], "{source}");
        }
        for source in [
            r#"const C = () => <motion.div whileInView="visible" viewport={{ once: true, amount: 0.1 }} />"#,
            // Built elsewhere: unknown, so not reported.
            r#"const C = () => <motion.div whileInView="visible" viewport={viewport} />"#,
        ] {
            assert_eq!(rules(source), Vec::<&str>::new(), "{source}");
        }
    }

    #[test]
    fn the_painted_section_stays_fixed_while_inner_content_moves() {
        for source in [
            r#"const C = () => <Box as={motion.section} bg="$surface" whileInView="visible" viewport={{ once: true }}><Title /></Box>"#,
            r#"const C = () => <motion.section className={css({ bg: '$surface', py: 20 })} whileInView="visible" viewport={{ once: true }} />"#,
        ] {
            assert_eq!(rules(source), ["reveal-moves-background"], "{source}");
        }
        let inner = r#"const C = () => <Box as="section" bg="$surface"><motion.div whileInView="visible" viewport={{ once: true }}><Title /></motion.div></Box>"#;
        assert_eq!(rules(inner), Vec::<&str>::new());
    }

    #[test]
    fn reduced_motion_is_asked_only_of_files_that_add_entrance_motion() {
        let reveal = r#"const C = () => <motion.div whileInView="visible" viewport={{ once: true }} />"#;
        assert_eq!(reduced_motion_finding(reveal).map(|finding| finding.rule), Some("motion-without-reduced-motion"));
        for source in [
            r#"const C = () => { const reduced = useReducedMotion(); return <motion.div whileInView={reduced ? undefined : "visible"} viewport={{ once: true }} /> }"#,
            r#"const C = () => <MotionConfig reducedMotion="user"><motion.div whileInView="visible" viewport={{ once: true }} /></MotionConfig>"#,
            // Hover and layout motion are not entrances.
            r#"const C = () => <motion.button whileHover={{ scale: 1.02 }} />"#,
            r#"const C = () => <Box bg="$surface" />"#,
        ] {
            assert!(reduced_motion_finding(source).is_none(), "{source}");
        }
        assert!(handles_reduced_motion("globalCss({ '@media (prefers-reduced-motion: reduce)': {} })"));
    }
}
