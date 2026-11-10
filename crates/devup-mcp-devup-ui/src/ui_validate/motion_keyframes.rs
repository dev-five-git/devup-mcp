//! Entrance motion written with Devup UI's static `keyframes()` and CSS
//! animation props, read the same way `motion_quality` reads framer-motion.
//!
//! A *style bag* is one element's styles as the source states them: the
//! props of a JSX element, or the object handed to `css()`/`styled()`. An
//! *entrance* is a bag whose animation names keyframes that start hidden
//! (`from: { opacity: 0 }`), or that move and are driven by a view timeline.
//! A looping animation (`infinite`) is a loader or a pulse, not an entrance.
use super::{BACKGROUND_PROPS, is_zero, property};
use oxc_ast::ast::*;
use oxc_span::Span;

/// What the first frame of a `keyframes()` call does.
#[derive(Clone, Copy, Default)]
pub(super) struct Frames {
    hides: bool,
    moves: bool,
}

/// `keyframes({ from: {…}, to: {…} })`, also as `Devup.keyframes(…)`.
fn keyframes_object<'b, 'a>(expression: &'b Expression<'a>) -> Option<&'b ObjectExpression<'a>> {
    let Expression::CallExpression(call) = expression.get_inner_expression() else {
        return None;
    };
    let named = match call.callee.get_inner_expression() {
        Expression::Identifier(callee) => callee.name == "keyframes",
        Expression::StaticMemberExpression(member) => member.property.name == "keyframes",
        _ => false,
    };
    match call.arguments.first() {
        Some(Argument::ObjectExpression(object)) if named => Some(object),
        _ => None,
    }
}

fn frames(object: &ObjectExpression<'_>) -> Frames {
    let Some(Expression::ObjectExpression(first)) = ["from", "0%"].iter().find_map(|key| property(object, key)) else {
        return Frames::default();
    };
    Frames {
        hides: property(first, "opacity").is_some_and(is_zero)
            || matches!(property(first, "visibility"), Some(Expression::StringLiteral(text)) if text.value == "hidden"),
        moves: ["transform", "translate", "scale"].iter().any(|key| property(first, key).is_some()),
    }
}

/// `const rise = keyframes({…})` declarations of this file, by name.
pub(super) struct Declared(pub Vec<(String, Frames)>);

impl Declared {
    pub(super) fn record(&mut self, node: &VariableDeclarator<'_>) {
        if let (Some(name), Some(object)) = (node.id.get_binding_identifier(), node.init.as_ref().and_then(keyframes_object)) {
            self.0.push((name.name.to_string(), frames(object)));
        }
    }

    /// The keyframes an `animationName`/`animation` value refers to: a
    /// declared name, an inline `keyframes()` call, or a template holding one.
    fn referenced(&self, expression: &Expression<'_>) -> Option<Frames> {
        match expression.get_inner_expression() {
            Expression::Identifier(name) => self.0.iter().find(|(declared, _)| declared == name.name.as_str()).map(|(_, frames)| *frames),
            Expression::TemplateLiteral(template) => template.expressions.iter().find_map(|part| self.referenced(part)),
            other => keyframes_object(other).map(frames),
        }
    }
}

struct Entry<'b, 'a> {
    key: &'b str,
    expression: Option<&'b Expression<'a>>,
    text: Option<&'b str>,
    span: Span,
}

/// One element's styles, from JSX props or from a style object.
pub(super) struct Bag<'b, 'a>(Vec<Entry<'b, 'a>>);

fn literal_text<'b>(expression: &'b Expression<'_>) -> Option<&'b str> {
    match expression {
        Expression::StringLiteral(text) => Some(text.value.as_str()),
        Expression::TemplateLiteral(template) if template.expressions.is_empty() => {
            template.quasis.first().map(|quasi| quasi.value.raw.as_str())
        }
        _ => None,
    }
}

impl<'b, 'a> Bag<'b, 'a> {
    pub(super) fn of_element(node: &'b JSXOpeningElement<'a>) -> Self {
        Self(
            node.attributes
                .iter()
                .filter_map(|item| match item {
                    JSXAttributeItem::Attribute(attribute) => Some(&**attribute),
                    JSXAttributeItem::SpreadAttribute(_) => None,
                })
                .filter_map(|attribute| {
                    let JSXAttributeName::Identifier(name) = &attribute.name else {
                        return None;
                    };
                    let expression = super::expression(attribute);
                    let text = match attribute.value.as_ref() {
                        Some(JSXAttributeValue::StringLiteral(value)) => Some(value.value.as_str()),
                        _ => expression.and_then(literal_text),
                    };
                    Some(Entry { key: name.name.as_str(), expression, text, span: attribute.span })
                })
                .collect(),
        )
    }

    pub(super) fn of_object(object: &'b ObjectExpression<'a>) -> Self {
        Self(
            object
                .properties
                .iter()
                .filter_map(|item| match item {
                    ObjectPropertyKind::ObjectProperty(entry) => Some(&**entry),
                    ObjectPropertyKind::SpreadProperty(_) => None,
                })
                .filter_map(|entry| {
                    let key = match &entry.key {
                        PropertyKey::StaticIdentifier(name) => name.name.as_str(),
                        PropertyKey::StringLiteral(name) => name.value.as_str(),
                        _ => return None,
                    };
                    let expression = entry.value.get_inner_expression();
                    Some(Entry { key, expression: Some(expression), text: literal_text(expression), span: entry.span })
                })
                .collect(),
        )
    }

    fn entry(&self, key: &str) -> Option<&Entry<'b, 'a>> {
        self.0.iter().find(|entry| entry.key == key)
    }

    fn text(&self, key: &str) -> Option<&'b str> {
        self.entry(key).and_then(|entry| entry.text)
    }

    /// Whether the bag states any animation at all (cheap pre-filter).
    pub(super) fn animates(&self) -> bool {
        self.0.iter().any(|entry| entry.key.starts_with("animation") || entry.key == "transitionDelay")
    }

    pub(super) fn paints_background(&self) -> bool {
        BACKGROUND_PROPS.iter().any(|name| self.entry(name).is_some())
    }

    fn loops(&self) -> bool {
        let shorthand = self.entry("animation").and_then(|entry| entry.expression).is_some_and(|expression| match expression {
            Expression::TemplateLiteral(template) => template.quasis.iter().any(|quasi| quasi.value.raw.contains("infinite")),
            Expression::StringLiteral(text) => text.value.contains("infinite"),
            _ => false,
        });
        shorthand || self.text("animationIterationCount") == Some("infinite")
    }

    fn view_timeline(&self) -> bool {
        self.text("animationTimeline").is_some_and(|timeline| timeline.contains("view("))
    }

    /// The entrance this bag declares, and where.
    pub(super) fn entrance(&self, declared: &Declared) -> Option<Entrance> {
        if self.loops() {
            return None;
        }
        let (span, frames) = ["animationName", "animation"].iter().find_map(|key| {
            let entry = self.entry(key)?;
            Some((entry.span, declared.referenced(entry.expression?)?))
        })?;
        let scroll = self.view_timeline();
        (frames.hides || (scroll && frames.moves)).then_some(Entrance { span, hides: frames.hides, scroll })
    }

    /// The resting style is hidden: `opacity: 0` or `visibility: hidden`
    /// stated on the element itself, outside any selector.
    pub(super) fn rests_hidden(&self) -> Option<Span> {
        let opacity = self.entry("opacity").filter(|entry| {
            entry.text.is_some_and(|text| text.trim() == "0") || entry.expression.is_some_and(is_zero)
        });
        opacity.or_else(|| self.entry("visibility").filter(|entry| entry.text == Some("hidden"))).map(|entry| entry.span)
    }

    /// A selector keyed on a marker that only a script sets (`[data-…]`, a
    /// class) is what shows the content again.
    fn shown_by_marker(&self) -> bool {
        let Some(Expression::ObjectExpression(selectors)) = self.entry("selectors").and_then(|entry| entry.expression) else {
            return false;
        };
        let nested = Bag::of_object(selectors);
        nested.0.iter().any(|entry| {
            let marker = entry.key.contains("[data-") || entry.key.contains("&.") || entry.key.starts_with('.');
            let shows = matches!(entry.expression, Some(Expression::ObjectExpression(style)) if {
                let style = Bag::of_object(style);
                style.entry("opacity").is_some_and(|opacity| !(opacity.text.is_some_and(|text| text.trim() == "0") || opacity.expression.is_some_and(is_zero)))
                    || style.text("visibility") == Some("visible")
                    || style.entry("animationName").is_some()
                    || style.entry("animation").is_some()
            });
            marker && shows
        })
    }

    /// `ssr-hidden-reveal` in keyframes form: where and why, when the
    /// content is invisible until something other than CSS shows it.
    pub(super) fn hidden_until_shown(&self, entrance: Option<&Entrance>) -> Option<(Span, &'static str)> {
        if let Some(span) = self.rests_hidden() {
            if entrance.is_some() {
                return Some((span, "This content rests at opacity 0 and appears only while its animation runs; reduced motion, a blocked stylesheet rule or an unsupported timeline leaves it invisible."));
            }
            if self.shown_by_marker() {
                return Some((span, "Server markup renders this content hidden until a script sets the attribute or class that shows it."));
            }
        }
        let paused = self.entry("animationPlayState").filter(|entry| entry.text == Some("paused"))?;
        entrance
            .filter(|entrance| entrance.hides)
            .map(|_| (paused.span, "This entrance is paused on its hidden first frame until a script resumes it."))
    }

    /// A view-timeline entrance must end early in the entry: with no range
    /// (or one that reaches the exit) the content is still part-transparent
    /// in the middle of the screen and fades out again as it leaves.
    pub(super) fn scroll_range_replays(&self, entrance: &Entrance) -> Option<Span> {
        if !entrance.scroll {
            return None;
        }
        let range = self.entry("animationRange").or_else(|| self.entry("animationRangeEnd"));
        match range {
            None => Some(entrance.span),
            // Built elsewhere: unknown, so not reported.
            Some(entry) => entry.text.filter(|text| !range_ends_early(text)).map(|_| entry.span),
        }
    }

    /// The per-item step of a staggered `animationDelay`/`transitionDelay`.
    pub(super) fn stagger_step(&self) -> Option<(Span, f64)> {
        ["animationDelay", "transitionDelay"].iter().find_map(|key| {
            let entry = self.entry(key)?;
            let step = match (entry.text, entry.expression) {
                (Some(text), _) => step_in_calc(text),
                (None, Some(Expression::TemplateLiteral(template))) => step_in_template(template),
                _ => None,
            }?;
            Some((entry.span, step))
        })
    }
}

pub(super) struct Entrance {
    pub span: Span,
    hides: bool,
    scroll: bool,
}

/// Whether an `animation-range` finishes while the block is still entering:
/// its last named range is `entry`, or `cover`/`contain`/a bare percentage of
/// at most 50%.
fn range_ends_early(range: &str) -> bool {
    let tokens: Vec<&str> = range.split_whitespace().collect();
    let named = |token: &&str| matches!(*token, "entry" | "entry-crossing" | "exit" | "exit-crossing" | "cover" | "contain" | "normal");
    let Some(index) = tokens.iter().rposition(named) else {
        return tokens.last().and_then(|token| percent(token)).is_some_and(|value| value <= 50.0);
    };
    match tokens[index] {
        "entry" | "entry-crossing" => true,
        "cover" | "contain" => tokens.get(index + 1).and_then(|token| percent(token)).is_some_and(|value| value <= 50.0),
        _ => false,
    }
}

fn percent(token: &str) -> Option<f64> {
    token.strip_suffix('%')?.parse().ok()
}

/// `60ms` / `0.06s`, in seconds.
fn duration(token: &str) -> Option<f64> {
    let token = token.trim();
    if let Some(number) = token.strip_suffix("ms") {
        return number.parse::<f64>().ok().map(|value| value / 1000.0);
    }
    token.strip_suffix('s')?.parse().ok()
}

fn is_duration_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '.'
}

/// `calc(var(--i) * 60ms)` or `calc(60ms * var(--i))`: the duration that is
/// multiplied by something only known per item. `#` stands for a template
/// expression.
fn step_in_calc(text: &str) -> Option<f64> {
    let (left, right) = text.split_once('*')?;
    let per_item = |side: &str| side.contains("var(") || side.contains('#');
    let after: String = right.trim_start().chars().take_while(|character| is_duration_char(*character)).collect();
    let before: String = {
        let trimmed = left.trim_end();
        let start = trimmed.rfind(|character: char| !is_duration_char(character)).map_or(0, |index| index + 1);
        trimmed[start..].to_owned()
    };
    match (duration(&after), duration(&before)) {
        (Some(step), _) if per_item(left) => Some(step),
        (_, Some(step)) if per_item(right) => Some(step),
        _ => None,
    }
}

/// `` `calc(${index} * 60ms)` `` or `` `${index * 60}ms` ``.
fn step_in_template(template: &TemplateLiteral<'_>) -> Option<f64> {
    let joined = template.quasis.iter().map(|quasi| quasi.value.raw.as_str()).collect::<Vec<_>>().join("#");
    if let Some(step) = step_in_calc(&joined) {
        return Some(step);
    }
    template.expressions.iter().enumerate().find_map(|(index, part)| {
        let Expression::BinaryExpression(binary) = part.get_inner_expression() else {
            return None;
        };
        if binary.operator != BinaryOperator::Multiplication {
            return None;
        }
        let number = match (binary.left.get_inner_expression(), binary.right.get_inner_expression()) {
            (Expression::NumericLiteral(number), other) | (other, Expression::NumericLiteral(number))
                if !matches!(other, Expression::NumericLiteral(_)) =>
            {
                number.value
            }
            _ => return None,
        };
        let unit = template.quasis.get(index + 1)?.value.raw.as_str();
        if unit.starts_with("ms") {
            Some(number / 1000.0)
        } else if unit.starts_with('s') {
            Some(number)
        } else {
            None
        }
    })
}

/// What an `IntersectionObserver` callback does with visibility.
#[derive(Default)]
pub(super) struct ObserverCallback {
    /// It calls `unobserve`/`disconnect`: one-shot.
    pub stops: bool,
    /// It writes `isIntersecting` straight into a class, attribute or style,
    /// so leaving the viewport undoes the entrance.
    pub mirrors_visibility: bool,
}

struct ReadsIntersecting(bool);

impl<'a> oxc_ast_visit::Visit<'a> for ReadsIntersecting {
    fn visit_identifier_name(&mut self, node: &IdentifierName<'a>) {
        self.0 |= node.name == "isIntersecting";
    }
}

fn reads_intersecting(expression: &Expression<'_>) -> bool {
    use oxc_ast_visit::Visit;
    let mut reads = ReadsIntersecting(false);
    reads.visit_expression(expression);
    reads.0
}

impl<'a> oxc_ast_visit::Visit<'a> for ObserverCallback {
    fn visit_identifier_name(&mut self, node: &IdentifierName<'a>) {
        self.stops |= matches!(node.name.as_str(), "unobserve" | "disconnect");
    }

    fn visit_call_expression(&mut self, node: &CallExpression<'a>) {
        let writes = matches!(node.callee.get_inner_expression(), Expression::StaticMemberExpression(member)
            if matches!(member.property.name.as_str(), "toggle" | "toggleAttribute" | "setAttribute" | "setProperty"));
        if writes {
            self.mirrors_visibility |= node.arguments.iter().filter_map(Argument::as_expression).any(reads_intersecting);
        }
        oxc_ast_visit::walk::walk_call_expression(self, node);
    }

    fn visit_assignment_expression(&mut self, node: &AssignmentExpression<'a>) {
        self.mirrors_visibility |= reads_intersecting(&node.right);
        oxc_ast_visit::walk::walk_assignment_expression(self, node);
    }
}

/// `new IntersectionObserver(callback, …)` with an inline callback.
pub(super) fn observer_callback(node: &NewExpression<'_>) -> Option<ObserverCallback> {
    use oxc_ast_visit::Visit;
    let named = match node.callee.get_inner_expression() {
        Expression::Identifier(callee) => callee.name == "IntersectionObserver",
        Expression::StaticMemberExpression(member) => member.property.name == "IntersectionObserver",
        _ => false,
    };
    if !named {
        return None;
    }
    let mut callback = ObserverCallback::default();
    match node.arguments.first()?.as_expression()?.get_inner_expression() {
        Expression::ArrowFunctionExpression(function) => callback.visit_arrow_function_expression(function),
        Expression::FunctionExpression(function) => callback.visit_function_body(function.body.as_ref()?),
        // A callback declared elsewhere cannot be read from here.
        _ => return None,
    }
    Some(callback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_ends_early_only_inside_the_entry() {
        for range in ["entry", "entry 0% entry 100%", "entry 0% entry min(100%, 240px)", "entry 0% cover 30%", "0% 40%", "entry-crossing"] {
            assert!(range_ends_early(range), "{range}");
        }
        for range in ["cover", "contain", "normal", "entry 0% exit 100%", "exit", "entry 0% cover 80%", "0% 100%"] {
            assert!(!range_ends_early(range), "{range}");
        }
    }

    #[test]
    fn a_per_item_step_is_a_duration_multiplied_by_an_index() {
        for (text, step) in [("calc(var(--i) * 60ms)", 0.06), ("calc(0.2s * var(--index))", 0.2), ("calc(# * 250ms)", 0.25)] {
            assert_eq!(step_in_calc(text), Some(step), "{text}");
        }
        for text in ["calc(2 * 60ms)", "200ms", "calc(var(--base) + 60ms)"] {
            assert_eq!(step_in_calc(text), None, "{text}");
        }
    }
}
