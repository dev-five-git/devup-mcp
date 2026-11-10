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
//!   plays again on every re-entry (warning). An in-view hook
//!   (`useInView` from motion or react-intersection-observer) with no
//!   one-shot option is the same question asked of code whose purpose is not
//!   visible here, so it is info.
//! - `reveal-moves-background`: the element that carries the section's paint
//!   is the one being faded or moved, so the background animates with it.
//! - `stagger-too-long`: each item of a group waits more than 150ms after
//!   the previous one (`staggerChildren`, or `delay: index * step`), so a
//!   handful of cards takes over a second to finish appearing.
//! - `motion-without-reduced-motion`: entrance motion with no reduced-motion
//!   handling anywhere in the validated sources (cross-file, see
//!   [`reduced_motion_finding`]).
//! - `entrance-uses-motion-library` (info, once per file): the entrance is
//!   driven by a runtime animation library where Devup UI's static keyframes
//!   would do it in CSS.
//! - `entrance-on-static-content`: navigation, a header or footer, a form or
//!   its fields, or a paragraph of body copy is given an entrance. Those are
//!   read or operated at once and get none.
//! - `entrance-per-element` (info): a heading or a control carries its own
//!   entrance; the entrance belongs to the content group around it.
//!
//! Every rule reads both styles: framer-motion props (`initial`,
//! `whileInView`, `viewport`, `transition`) and Devup UI keyframes
//! (`keyframes()`, `animationName`/`animation`, `animationTimeline`,
//! `animationRange`, `animationDelay`, selectors and a plain
//! `IntersectionObserver`), see [`keyframes`].
#[path = "motion_keyframes.rs"]
mod keyframes;

use super::{Severity, Violation, site};
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::SourceType;

/// The longest per-item step, in seconds, that still reads as one group.
const MAX_STAGGER_STEP: f64 = 0.15;

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
        "Leave content in place for reduced motion: _motionReduce={{ animation: 'none' }} on the animated element (Devup UI), a prefers-reduced-motion media rule, or useReducedMotion()/<MotionConfig reducedMotion=\"user\"> for a motion library. Validate the shared rule or provider together with this file if it lives elsewhere.",
    ))
}

/// Whether this source reads the user's motion preference or configures it
/// for the tree: a reference to `useReducedMotion`/`usePrefersReducedMotion`
/// (or any `…ReducedMotion` name), a `reducedMotion`/`_motionReduce`/
/// `_motionSafe` prop or key, or a `prefers-reduced-motion` media rule or
/// query in a string.
///
/// Read from the syntax tree, so a comment that mentions reduced motion or an
/// import nobody calls does not count as handling it.
pub fn handles_reduced_motion(source: &str) -> bool {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    if parsed.panicked || !parsed.diagnostics.is_empty() {
        // Unparseable source is reported as invalid-syntax elsewhere; do not
        // add a motion finding on top of it.
        return true;
    }
    let mut preference = Preference(false);
    preference.visit_program(&parsed.program);
    preference.0
}

struct Preference(bool);

impl Preference {
    fn name(&mut self, name: &str) {
        self.0 |= name.contains("ReducedMotion") || matches!(name, "reducedMotion" | "_motionReduce" | "_motionSafe");
    }
    fn text(&mut self, text: &str) {
        self.0 |= text.contains("prefers-reduced-motion");
    }
}

impl<'a> Visit<'a> for Preference {
    // An import names the hook; only a use of it handles anything.
    fn visit_import_declaration(&mut self, _: &ImportDeclaration<'a>) {}
    fn visit_identifier_reference(&mut self, node: &IdentifierReference<'a>) {
        self.name(node.name.as_str());
    }
    fn visit_identifier_name(&mut self, node: &IdentifierName<'a>) {
        self.name(node.name.as_str());
    }
    fn visit_jsx_identifier(&mut self, node: &JSXIdentifier<'a>) {
        self.name(node.name.as_str());
    }
    fn visit_string_literal(&mut self, node: &StringLiteral<'a>) {
        self.text(node.value.as_str());
    }
    fn visit_template_element(&mut self, node: &TemplateElement<'a>) {
        self.text(node.value.raw.as_str());
    }
}

fn collect(source: &str) -> Option<Collector<'_>> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    if parsed.panicked || !parsed.diagnostics.is_empty() {
        return None;
    }
    let mut components = MotionComponents(Vec::new(), keyframes::Declared(Vec::new()));
    components.visit_program(&parsed.program);
    let mut collector = Collector {
        source,
        findings: Vec::new(),
        first_entrance: None,
        optional: 0,
        motion_components: components.0,
        declared: components.1,
        library_noted: false,
    };
    collector.visit_program(&parsed.program);
    Some(collector)
}

fn is_motion_namespace(name: &str) -> bool {
    matches!(name, "motion" | "m")
}

/// `const MotionBox = motion.create(Box)` / `motion(Box)`: a component that
/// takes the same `initial`/`whileInView` props as `motion.div`. The same
/// pass records every `const name = keyframes({…})`.
struct MotionComponents(Vec<String>, keyframes::Declared);

impl<'a> Visit<'a> for MotionComponents {
    fn visit_variable_declarator(&mut self, node: &VariableDeclarator<'a>) {
        self.1.record(node);
        if let (Some(name), Some(Expression::CallExpression(call))) =
            (node.id.get_binding_identifier(), node.init.as_ref().map(Expression::get_inner_expression))
        {
            let wraps = match call.callee.get_inner_expression() {
                Expression::Identifier(callee) => is_motion_namespace(callee.name.as_str()),
                Expression::StaticMemberExpression(member) => {
                    member.property.name == "create"
                        && matches!(member.object.get_inner_expression(), Expression::Identifier(object) if is_motion_namespace(object.name.as_str()))
                }
                _ => false,
            };
            if wraps {
                self.0.push(name.name.to_string());
            }
        }
        walk::walk_variable_declarator(self, node);
    }
}

struct Collector<'s> {
    source: &'s str,
    findings: Vec<Violation>,
    first_entrance: Option<oxc_span::Span>,
    /// Depth inside `AnimatePresence` or a conditional render: such an
    /// element mounts on the client later, so its initial style is not
    /// server markup.
    optional: usize,
    /// Components this file builds with `motion.create(…)`/`motion(…)`.
    motion_components: Vec<String>,
    /// `keyframes()` constants of this file.
    declared: keyframes::Declared,
    library_noted: bool,
}

const KEYFRAMES_FIX: &str = "Declare the entrance once with Devup UI keyframes (from: { opacity: 0, transform }) and put animationName, animationFillMode=\"both\", animationTimeline=\"view()\" and animationRange=\"entry 0% entry min(100%, 240px)\" on an inner wrapper: the hidden start then lives only in CSS, and content shows without JavaScript.";
const MOVES_BACKGROUND: &str = "The element that paints the section background is the one being revealed, so the background fades or moves with it.";

impl Collector<'_> {
    fn report(&mut self, span: oxc_span::Span, rule: &'static str, severity: Severity, message: &str, help: &'static str) {
        self.findings.push(site::violation(self.source, span, rule, severity, message.into(), help));
    }

    /// What the role table of the animation guide excludes, proven from the
    /// element itself.
    fn check_role(&mut self, span: oxc_span::Span, role: &str) {
        if matches!(role, "nav" | "header" | "footer" | "form" | "input" | "textarea" | "select" | "label" | "p") {
            self.report(span, "entrance-on-static-content", Severity::Warning,
                "Navigation, headers, footers, forms and body copy are read or operated at once; this element is given an entrance.",
                "Remove the entrance here. Reveal the content group of a below-fold section instead, and leave what the reader uses immediately in place. Keep it only when the design explicitly asks for it.");
        } else if matches!(role, "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "button" | "a") {
            self.report(span, "entrance-per-element", Severity::Info,
                "This heading or control carries its own entrance.",
                "One entrance per content group is the normal case: put it on the wrapper around the heading, text and controls rather than on each of them.");
        }
    }

    /// Keyframes-style checks for one element's styles. `painted` says the
    /// element (or its class) paints a background. Returns whether the
    /// styles declare an entrance.
    fn check_bag(&mut self, bag: &keyframes::Bag<'_, '_>, painted: bool) -> bool {
        let entrance = bag.entrance(&self.declared);
        if let Some(entrance) = &entrance {
            self.first_entrance.get_or_insert(entrance.span);
            if let Some(span) = bag.scroll_range_replays(entrance) {
                self.report(span, "reveal-replays", Severity::Warning,
                    "This view-timeline entrance has no range that ends while the block is entering, so the content is still part-transparent in the middle of the screen and fades out again as it leaves.",
                    "Add animationRange=\"entry 0% entry min(100%, 240px)\" (or another range that ends inside the entry, at most cover 50%). Keep a full-pass range only when the design explicitly asks for it.");
            }
            if painted {
                self.report(entrance.span, "reveal-moves-background", Severity::Warning, MOVES_BACKGROUND,
                    "Keep the painted section fixed and put the animation props on an inner content wrapper.");
            }
        }
        if self.optional == 0
            && let Some((span, message)) = bag.hidden_until_shown(entrance.as_ref())
        {
            self.report(span, "ssr-hidden-reveal", Severity::Warning, message,
                "Keep the resting style visible and state the hidden start only in the keyframes' first frame (animationFillMode=\"both\"). A fallback script may mark content that is still below the viewport, never content already rendered in view. Check no-JavaScript, reduced motion, reload and client navigation.");
        }
        if let Some((span, step)) = bag.stagger_step().filter(|(_, step)| *step > MAX_STAGGER_STEP) {
            self.stagger(span, step);
        }
        entrance.is_some()
    }

    fn stagger(&mut self, span: oxc_span::Span, step: f64) {
        self.findings.push(site::violation(self.source, span, "stagger-too-long", Severity::Warning,
            format!("Each item waits {}ms after the previous one, so the group takes a long time to finish appearing.", (step * 1000.0).round()),
            "Keep a group stagger short (40-80ms per item, the whole group within about 400ms), or let the group enter together. Keep a longer step only when the design explicitly asks for it."));
    }

    /// Once per file: this entrance needs a runtime library.
    fn note_library(&mut self, span: oxc_span::Span) {
        if !std::mem::replace(&mut self.library_noted, true) {
            self.report(span, "entrance-uses-motion-library", Severity::Info,
                "This entrance is driven by a runtime animation library; Devup UI keyframes produce the same motion as static CSS, with no script to download or hydrate.",
                KEYFRAMES_FIX);
        }
    }
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

fn is_motion_element(node: &JSXOpeningElement<'_>, components: &[String]) -> bool {
    match &node.name {
        JSXElementName::MemberExpression(member) => {
            matches!(&member.object, JSXMemberExpressionObject::IdentifierReference(object) if is_motion_namespace(object.name.as_str()))
        }
        name => name
            .get_identifier_name()
            .is_some_and(|name| components.iter().any(|component| component == name.as_str())),
    }
}

/// Whether an in-view hook call is one-shot: `Some(true)` with
/// `once: true` (motion) or `triggerOnce: true` (react-intersection-observer),
/// `Some(false)` when every argument is readable and none says so, `None`
/// when options are built elsewhere.
fn hook_is_one_shot(call: &CallExpression<'_>) -> Option<bool> {
    let mut readable = true;
    for (index, argument) in call.arguments.iter().enumerate() {
        match argument.as_expression().map(Expression::get_inner_expression) {
            Some(Expression::ObjectExpression(object)) => {
                if ["once", "triggerOnce"].iter().any(|key| {
                    matches!(property(object, key), Some(Expression::BooleanLiteral(flag)) if flag.value)
                }) {
                    return Some(true);
                }
                readable &= !object
                    .properties
                    .iter()
                    .any(|item| matches!(item, ObjectPropertyKind::SpreadProperty(_)));
            }
            // motion's first argument is the element ref.
            Some(Expression::Identifier(_)) if index == 0 => {}
            _ => readable = false,
        }
    }
    readable.then_some(false)
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

/// What the element is: `as="nav"`, an intrinsic tag (`<footer>`), or the
/// tag of a motion element (`motion.p`).
fn element_role<'b>(node: &'b JSXOpeningElement<'_>) -> Option<&'b str> {
    if let Some(role) = attribute(node, "as").and_then(string) {
        return Some(role);
    }
    match &node.name {
        JSXElementName::Identifier(name) => Some(name.name.as_str()),
        JSXElementName::MemberExpression(member) => Some(member.property.name.as_str()),
        _ => None,
    }
}

/// The object of `className={css({ … })}`.
fn class_object<'b, 'a>(node: &'b JSXOpeningElement<'a>) -> Option<&'b ObjectExpression<'a>> {
    match attribute(node, "className").and_then(expression)? {
        Expression::CallExpression(call) => match call.arguments.first()? {
            Argument::ObjectExpression(object) => Some(object),
            _ => None,
        },
        _ => None,
    }
}

fn paints_background(node: &JSXOpeningElement<'_>) -> bool {
    BACKGROUND_PROPS.iter().any(|name| attribute(node, name).is_some())
        || class_object(node).is_some_and(|object| BACKGROUND_PROPS.iter().any(|name| property(object, name).is_some()))
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

    fn visit_object_property(&mut self, node: &ObjectProperty<'a>) {
        // `staggerChildren: 0.3`, or `delay: index * 0.3` (either order). A
        // fixed `delay: 0.3` is one wait, not a per-item step.
        let step = match (node.key.static_name().as_deref(), node.value.get_inner_expression()) {
            (Some("staggerChildren"), Expression::NumericLiteral(number)) => Some(number.value),
            (Some("delay"), Expression::BinaryExpression(binary)) if binary.operator == BinaryOperator::Multiplication => {
                match (binary.left.get_inner_expression(), binary.right.get_inner_expression()) {
                    (Expression::NumericLiteral(number), other) | (other, Expression::NumericLiteral(number))
                        if !matches!(other, Expression::NumericLiteral(_)) =>
                    {
                        Some(number.value)
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(step) = step.filter(|step| *step > MAX_STAGGER_STEP) {
            self.stagger(node.span, step);
        }
        walk::walk_object_property(self, node);
    }

    fn visit_object_expression(&mut self, node: &ObjectExpression<'a>) {
        // A style object (`css({…})`, `styled.div({…})`, a selector's value).
        let bag = keyframes::Bag::of_object(node);
        if bag.animates() {
            let painted = bag.paints_background();
            let _ = self.check_bag(&bag, painted);
        }
        walk::walk_object_expression(self, node);
    }

    fn visit_new_expression(&mut self, node: &NewExpression<'a>) {
        if let Some(callback) = keyframes::observer_callback(node).filter(|callback| !callback.stops) {
            let (severity, message) = if callback.mirrors_visibility {
                (Severity::Warning, "This observer writes isIntersecting straight into the element, so leaving the viewport undoes the entrance and re-entering replays it.")
            } else {
                (Severity::Info, "This observer never calls unobserve() or disconnect(); an entrance driven by it replays on every re-entry.")
            };
            self.report(node.span, "reveal-replays", severity, message,
                "For an entrance, mark the element once and then unobserve it. Leave it as is when it tracks visibility for another purpose.");
        }
        walk::walk_new_expression(self, node);
    }

    fn visit_call_expression(&mut self, node: &CallExpression<'a>) {
        if matches!(node.callee.get_inner_expression(), Expression::Identifier(callee) if callee.name == "useInView")
            && hook_is_one_shot(node) == Some(false)
        {
            self.findings.push(site::violation(self.source, node.span, "reveal-replays", Severity::Info,
                "This in-view hook has no one-shot option; an entrance driven by it replays on every re-entry.".into(),
                "For an entrance, pass { once: true } (motion) or { triggerOnce: true } (react-intersection-observer). Leave it as is when it tracks visibility for another purpose."));
        }
        walk::walk_call_expression(self, node);
    }

    fn visit_jsx_opening_element(&mut self, node: &JSXOpeningElement<'a>) {
        let in_view = attribute(node, "whileInView");
        let hidden = is_motion_element(node, &self.motion_components) && starts_hidden(node);
        if (in_view.is_some() || hidden) && self.first_entrance.is_none() {
            self.first_entrance = Some(node.span);
        }
        // An exit/enter pair inside AnimatePresence is state motion that CSS
        // keyframes do not replace; only an always-rendered entrance is noted.
        if in_view.is_some() || (hidden && self.optional == 0) {
            self.note_library(node.span);
        }
        // The same element in keyframes form: its own props, with the
        // background its class paints.
        let bag = keyframes::Bag::of_element(node);
        let class_entrance = class_object(node).and_then(|object| keyframes::Bag::of_object(object).entrance(&self.declared)).map(|entrance| entrance.span);
        let mut enters = in_view.is_some() || (hidden && self.optional == 0) || class_entrance.is_some();
        if bag.animates() || bag.rests_hidden().is_some() {
            let painted = paints_background(node);
            enters |= self.check_bag(&bag, painted);
        } else if BACKGROUND_PROPS.iter().any(|name| attribute(node, name).is_some())
            && let Some(span) = class_entrance
        {
            // The class animates and the element's own props paint.
            self.report(span, "reveal-moves-background", Severity::Warning, MOVES_BACKGROUND,
                "Keep the painted section fixed and put the animation props on an inner content wrapper.");
        }
        if enters && let Some(role) = element_role(node) {
            self.check_role(node.span, role);
        }
        if hidden && self.optional == 0 {
            self.findings.push(site::violation(self.source, node.span, "ssr-hidden-reveal", Severity::Warning,
                "Server markup renders this content at opacity 0 until JavaScript animates it.".into(),
                KEYFRAMES_FIX));
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
#[path = "motion_keyframes_tests.rs"]
mod keyframes_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// The rules found, without the once-per-file library note (it has its
    /// own test in `motion_keyframes_tests.rs`).
    pub(super) fn rules(source: &str) -> Vec<&'static str> {
        findings(source).into_iter().map(|finding| finding.rule).filter(|rule| *rule != "entrance-uses-motion-library").collect()
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
    fn components_built_from_motion_are_motion_elements() {
        for source in [
            r#"const MotionBox = motion.create(Box); const C = () => <MotionBox initial={{ opacity: 0 }} animate={{ opacity: 1 }} />"#,
            r#"const C = () => <Card initial={{ opacity: 0 }} animate={{ opacity: 1 }} />; const Card = motion(Box)"#,
            r#"const Fade = m.create('div'); const C = () => <Fade initial={{ opacity: 0 }} animate={{ opacity: 1 }} />"#,
        ] {
            assert_eq!(rules(source), ["ssr-hidden-reveal"], "{source}");
        }
        // Built by something else: `initial` is not known to be motion's.
        let other = r#"const Card = styled(Box); const C = () => <Card initial={{ opacity: 0 }} />"#;
        assert_eq!(rules(other), Vec::<&str>::new());
    }

    #[test]
    fn an_in_view_hook_without_a_one_shot_option_is_information() {
        for source in [
            r#"function C() { const ref = useRef(null); const inView = useInView(ref); return <div ref={ref} /> }"#,
            r#"function C() { const inView = useInView(ref, { amount: 0.2 }); return null }"#,
            r#"function C() { const { ref, inView } = useInView({ threshold: 0.2 }); return <div ref={ref} /> }"#,
        ] {
            let found = findings(source);
            assert_eq!(found.len(), 1, "{source}");
            assert_eq!((found[0].rule, found[0].severity), ("reveal-replays", Severity::Info), "{source}");
        }
        for source in [
            r#"function C() { const inView = useInView(ref, { once: true, amount: 0.1 }); return null }"#,
            r#"function C() { const { ref, inView } = useInView({ triggerOnce: true }); return <div ref={ref} /> }"#,
            // Options built elsewhere cannot be read from here.
            r#"function C() { const inView = useInView(ref, options); return null }"#,
            r#"function C() { const inView = useInView(ref, { ...defaults }); return null }"#,
            r#"function C() { const inView = other.useInView(ref); return null }"#,
        ] {
            assert_eq!(rules(source), Vec::<&str>::new(), "{source}");
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
    fn a_group_stagger_stays_short() {
        for source in [
            r#"const list = { visible: { transition: { staggerChildren: 0.3 } } }"#,
            r#"const C = () => items.map((item, index) => <motion.li key={item.id} animate={{ y: 0 }} transition={{ delay: index * 0.25 }} />)"#,
            r#"const C = () => items.map((item, i) => <motion.li key={item.id} animate={{ y: 0 }} transition={{ delay: 0.2 * i }} />)"#,
        ] {
            assert_eq!(rules(source), ["stagger-too-long"], "{source}");
        }
        for source in [
            r#"const list = { visible: { transition: { staggerChildren: 0.06 } } }"#,
            r#"const C = () => items.map((item, index) => <motion.li key={item.id} animate={{ y: 0 }} transition={{ delay: index * 0.08 }} />)"#,
            // One fixed wait, and arithmetic that is not a per-item step.
            r#"const C = () => <motion.div animate={{ y: 0 }} transition={{ delay: 0.4 }} />"#,
            r#"const C = () => <motion.div animate={{ y: 0 }} transition={{ delay: 2 * 0.4 }} />"#,
            r#"const C = () => <motion.div animate={{ y: 0 }} transition={{ delay: base + 0.4 }} />"#,
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
        let reveal = r#"// reduced motion: later
const C = () => <motion.div whileInView="visible" viewport={{ once: true }} />"#;
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
        for source in [
            "globalCss({ '@media (prefers-reduced-motion: reduce)': {} })",
            "const query = matchMedia(`(prefers-reduced-motion: reduce)`)",
            r#"const C = () => <Box _motionReduce={{ transform: 'none' }} />"#,
            "const transition = { reducedMotion: 'user' }",
        ] {
            assert!(handles_reduced_motion(source), "{source}");
        }
        // Mentioning it is not handling it.
        for source in [
            "// TODO: respect prefers-reduced-motion and useReducedMotion()\nconst a = 1",
            "import { useReducedMotion } from 'motion/react'\nconst a = 1",
        ] {
            assert!(!handles_reduced_motion(source), "{source}");
        }
    }
}
