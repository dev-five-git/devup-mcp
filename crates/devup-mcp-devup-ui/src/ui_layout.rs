//! Layout risks in devup-ui TSX that only show once a container is not the
//! width the code was written at, read statically from the props.
//!
//! Generated code and hand-edited code drift apart here: a consumer widening a
//! flat mobile screen re-anchors some boxes and not others, and a badge's
//! background ends up held right while its text is still held left. Both read
//! correctly at the width they were written at. This reads every positioned
//! sibling group, works out which edge each box is held by, and reports a box
//! drawn inside a painted box that is held by a different edge, and a straight
//! rule drawn as a mask in a box that stretches. It measures nothing: wrapping,
//! overlap and rendered geometry stay runtime questions.

use std::collections::BTreeMap;

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Expression, JSXAttributeItem, JSXAttributeName, JSXAttributeValue, JSXChild, JSXElement,
    JSXOpeningElement,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use serde_json::{Value, json};

const EDGE_TOLERANCE: f64 = 0.5;

#[derive(Debug, Clone)]
struct Element {
    ordinal: usize,
    tag: String,
    start: usize,
    attrs: BTreeMap<String, String>,
    has_children: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Anchor {
    Left,
    Right,
    Center,
    Stretch,
}

impl Anchor {
    fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Center => "center",
            Self::Stretch => "stretch",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Placement {
    anchor: Anchor,
    left: Option<f64>,
    right: Option<f64>,
    top: Option<f64>,
    height: Option<f64>,
}

/// Static layout risks for one TSX source. `reference_width` stands in for a
/// parent whose width the code does not state in px.
pub fn layout_risks(tsx: &str, reference_width: Option<f64>) -> Value {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, tsx, SourceType::tsx()).parse();
    let mut collector = Collector {
        source: tsx,
        reference_width,
        next: 0,
        stack: Vec::new(),
        risks: Vec::new(),
        unmeasured_groups: 0,
    };
    collector.visit_program(&parsed.program);
    json!({
        "risks": collector.risks,
        "groupsWithoutReferenceWidth": collector.unmeasured_groups,
        "checked": ["positioned siblings drawn inside a painted sibling, by the edge each is held by", "straight rules drawn as a mask in a box that stretches"],
        "runtimeUnverified": ["rendered geometry", "text wrapping and line breaks", "overlap between text and shapes", "font metrics"],
    })
}

struct Collector<'s> {
    source: &'s str,
    reference_width: Option<f64>,
    next: usize,
    stack: Vec<Vec<Element>>,
    risks: Vec<Value>,
    unmeasured_groups: usize,
}

impl<'a> Visit<'a> for Collector<'_> {
    fn visit_jsx_element(&mut self, element: &JSXElement<'a>) {
        let info = Element {
            ordinal: self.next,
            tag: self.source[element.opening_element.name.span().start as usize
                ..element.opening_element.name.span().end as usize]
                .to_owned(),
            start: element.opening_element.span.start as usize,
            attrs: string_attributes(&element.opening_element),
            has_children: element.children.iter().any(
                |child| !matches!(child, JSXChild::Text(text) if text.value.trim().is_empty()),
            ),
        };
        if let Some(siblings) = self.stack.last_mut() {
            siblings.push(info.clone());
        }
        self.stack.push(Vec::new());
        walk::walk_jsx_element(self, element);
        let children = self.stack.pop().unwrap_or_default();
        self.analyze(&info, &children);
    }

    fn visit_jsx_opening_element(&mut self, element: &JSXOpeningElement<'a>) {
        self.next += 1;
        walk::walk_jsx_opening_element(self, element);
    }
}

fn string_attributes(element: &JSXOpeningElement<'_>) -> BTreeMap<String, String> {
    element
        .attributes
        .iter()
        .filter_map(|item| {
            let JSXAttributeItem::Attribute(attribute) = item else {
                return None;
            };
            let JSXAttributeName::Identifier(name) = &attribute.name else {
                return None;
            };
            let value = match attribute.value.as_ref()? {
                JSXAttributeValue::StringLiteral(text) => text.value.to_string(),
                JSXAttributeValue::ExpressionContainer(container) => {
                    match container.expression.as_expression()?.get_inner_expression() {
                        Expression::StringLiteral(text) => text.value.to_string(),
                        Expression::NumericLiteral(number) => number.value.to_string(),
                        _ => return None,
                    }
                }
                _ => return None,
            };
            Some((name.name.to_string(), value))
        })
        .collect()
}

fn pixels(value: Option<&String>) -> Option<f64> {
    let value = value?.trim();
    value
        .strip_suffix("px")
        .unwrap_or(if value == "0" { "0" } else { "" })
        .parse()
        .ok()
}

fn center_offset(left: &str) -> Option<f64> {
    let left = left.trim();
    if left == "50%" {
        return Some(0.0);
    }
    let inner = left.strip_prefix("calc(50%")?.strip_suffix(')')?.trim();
    let (sign, rest) = inner.split_at(1);
    let amount = rest.trim().strip_suffix("px")?.trim().parse::<f64>().ok()?;
    match sign {
        "+" => Some(amount),
        "-" => Some(-amount),
        _ => None,
    }
}

impl Collector<'_> {
    fn analyze(&mut self, parent: &Element, children: &[Element]) {
        let width = pixels(parent.attrs.get("w"))
            .or_else(|| pixels(parent.attrs.get("boxSize")))
            .or(self.reference_width);
        let placed = children
            .iter()
            .filter(|child| child.attrs.get("pos").map(String::as_str) == Some("absolute"))
            .filter_map(|child| Some((child, self.place(child, width)?)))
            .collect::<Vec<_>>();
        for (child, placement) in &placed {
            self.check_line_mask(child, placement);
        }
        let mut unmeasured = false;
        for (index, (node, node_place)) in placed.iter().enumerate() {
            let container = placed[..index]
                .iter()
                .filter(|(candidate, _)| {
                    !candidate.has_children && candidate.attrs.contains_key("bg")
                })
                .filter_map(|(candidate, place)| {
                    let inside = inside(place, node_place)?;
                    Some((candidate, place, inside))
                })
                .min_by(|(_, left, _), (_, right, _)| area(left).total_cmp(&area(right)));
            let Some((container, container_place, full)) = container else {
                if width.is_none()
                    && placed[..index]
                        .iter()
                        .any(|(_, place)| place.anchor != node_place.anchor)
                {
                    unmeasured = true;
                }
                continue;
            };
            if container_place.anchor == node_place.anchor
                || container_place.anchor == Anchor::Stretch
            {
                continue;
            }
            self.risks.push(json!({
                "rule": "anchor-mismatch",
                "confidence": if full { "high" } else { "medium" },
                "referenceWidth": width,
                "elements": [self.describe(container, container_place), self.describe(node, node_place)],
                "message": format!(
                    "{} is drawn inside {} but held by its {} edge while the container is held by its {}; the two part once the parent is not {} wide. Hold both by the same edge.",
                    describe_short(node),
                    describe_short(container),
                    node_place.anchor.name(),
                    container_place.anchor.name(),
                    width.map_or_else(|| "the written width".to_owned(), |w| format!("{w}px")),
                ),
            }));
        }
        if unmeasured {
            self.unmeasured_groups += 1;
        }
    }

    fn place(&self, element: &Element, width: Option<f64>) -> Option<Placement> {
        let attrs = &element.attrs;
        let left_value = attrs.get("left");
        let right_value = attrs.get("right");
        let own_width = pixels(attrs.get("w")).or_else(|| pixels(attrs.get("boxSize")));
        let centred = attrs.get("transform").is_some_and(|transform| {
            transform.contains("translateX(-50%)") || transform.starts_with("translate(-50%,")
        });
        let anchor = match (left_value, right_value) {
            (Some(_), Some(_)) => Anchor::Stretch,
            (Some(left), None) if center_offset(left).is_some() => Anchor::Center,
            (Some(_), None) => Anchor::Left,
            (None, Some(_)) => Anchor::Right,
            (None, None) => return None,
        };
        let (left, right) = match anchor {
            Anchor::Left => {
                let left = pixels(left_value)?;
                (Some(left), own_width.map(|w| left + w))
            }
            Anchor::Right => {
                let edge = width.zip(pixels(right_value)).map(|(w, right)| w - right);
                (edge.zip(own_width).map(|(edge, w)| edge - w), edge)
            }
            Anchor::Center => {
                // `left: calc(50% + d)` places the left edge relative to the
                // centre; with `translateX(-50%)` it places the middle.
                let offset = left_value
                    .and_then(|left| center_offset(left))
                    .unwrap_or(0.0);
                let shift = if centred { 0.5 } else { 0.0 };
                let start = width
                    .zip(own_width)
                    .map(|(parent, own)| parent / 2.0 + offset - own * shift);
                (start, start.zip(own_width).map(|(start, own)| start + own))
            }
            Anchor::Stretch => (
                pixels(left_value),
                width.zip(pixels(right_value)).map(|(w, right)| w - right),
            ),
        };
        Some(Placement {
            anchor,
            left,
            right,
            top: pixels(attrs.get("top")),
            height: pixels(attrs.get("h")).or_else(|| pixels(attrs.get("boxSize"))),
        })
    }

    fn check_line_mask(&mut self, element: &Element, placement: &Placement) {
        let attrs = &element.attrs;
        let thin = pixels(attrs.get("h")).is_some_and(|height| height > 0.0 && height <= 2.0);
        let fixed_mask = attrs.get("maskSize").is_some_and(|size| {
            size == "contain" || size.split_whitespace().all(|part| part.ends_with("px"))
        });
        let follows = placement.anchor == Anchor::Stretch
            || attrs
                .get("w")
                .is_some_and(|w| w.contains('%') || w.contains("calc("));
        if attrs.contains_key("maskImage") && thin && fixed_mask && follows {
            self.risks.push(json!({
                "rule": "fixed-length-line-mask",
                "confidence": "high",
                "elements": [self.describe(element, placement)],
                "message": "A rule drawn as a mask keeps its drawn length while this box follows its parent's width, so the line stops short of the box at any other width. Size the mask to the box, or draw the rule as a filled box of the same colour and weight.",
            }));
        }
    }

    fn describe(&self, element: &Element, placement: &Placement) -> Value {
        let prefix = &self.source[..element.start.min(self.source.len())];
        json!({
            "elementIndex": element.ordinal,
            "element": element.tag,
            "anchor": placement.anchor.name(),
            "line": prefix.bytes().filter(|byte| *byte == b'\n').count() + 1,
            "column": prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1,
        })
    }
}

fn describe_short(element: &Element) -> String {
    format!("<{}> #{}", element.tag, element.ordinal)
}

fn area(place: &Placement) -> f64 {
    match (place.left, place.right, place.height) {
        (Some(left), Some(right), Some(height)) => (right - left) * height,
        _ => f64::INFINITY,
    }
}

/// Whether `node` lies inside `container` on both axes: `Some(true)` when every
/// edge of `node` is known and inside, `Some(false)` when only the known ones
/// are, `None` when it is outside or the container's box is not fully known.
fn inside(container: &Placement, node: &Placement) -> Option<bool> {
    let (c_left, c_right, c_top, c_height) = (
        container.left?,
        container.right?,
        container.top?,
        container.height?,
    );
    let top = node.top?;
    if top + EDGE_TOLERANCE < c_top || top > c_top + c_height + EDGE_TOLERANCE {
        return None;
    }
    let within = |x: f64| x + EDGE_TOLERANCE >= c_left && x <= c_right + EDGE_TOLERANCE;
    match (node.left, node.right) {
        (Some(left), Some(right)) => (within(left) && within(right)).then_some(true),
        (Some(edge), None) | (None, Some(edge)) => within(edge).then_some(false),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn risks(tsx: &str, width: Option<f64>) -> Vec<Value> {
        layout_risks(tsx, width)["risks"]
            .as_array()
            .unwrap()
            .clone()
    }

    #[test]
    fn a_badge_background_held_right_with_its_text_held_left_is_reported() {
        let tsx = r#"<Box h="760px" pos="relative" w="100%">
  <Box bg="$surface" h="22px" pos="absolute" right="36px" top="210px" w="64.11px" />
  <Text left="269.44px" pos="absolute" top="214px">20 left</Text>
</Box>"#;
        let found = risks(tsx, Some(360.0));
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0]["rule"], "anchor-mismatch");
        assert_eq!(found[0]["confidence"], "medium");
        assert_eq!(found[0]["elements"][0]["anchor"], "right");
        assert_eq!(found[0]["elements"][1]["anchor"], "left");
        assert_eq!(found[0]["elements"][1]["elementIndex"], 2);
    }

    #[test]
    fn a_centred_card_with_left_held_rows_is_reported_and_a_stretched_one_is_not() {
        let centred = r##"<Box h="760px" pos="relative" w="360px">
  <Box bg="#FFF" h="136px" left="calc(50% - 160px)" pos="absolute" top="193px" w="320px" />
  <Text left="36px" pos="absolute" top="209px" w="120px">Row</Text>
</Box>"##;
        let found = risks(centred, None);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0]["confidence"], "high");
        assert_eq!(found[0]["elements"][0]["anchor"], "center");

        let stretched = centred.replace(
            r#"left="calc(50% - 160px)" pos="absolute" top="193px" w="320px""#,
            r#"left="20px" pos="absolute" right="20px" top="193px""#,
        );
        assert!(risks(&stretched, None).is_empty(), "{stretched}");
    }

    #[test]
    fn a_rule_mask_in_a_stretching_box_is_reported() {
        let tsx = r#"<Box pos="relative" w="100%">
  <Box bg="$border" h="1px" left="35.5px" maskImage="url(/icons/Vector-1-164.svg)" maskPos="center" maskRepeat="no-repeat" maskSize="contain" pos="absolute" top="360.5px" w="calc(100% - 71px)" />
</Box>"#;
        let found = risks(tsx, Some(360.0));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0]["rule"], "fixed-length-line-mask");
        let fixed = tsx.replace("w=\"calc(100% - 71px)\"", "w=\"289px\"");
        assert!(risks(&fixed, Some(360.0)).is_empty());
    }

    #[test]
    fn without_a_width_mixed_anchors_are_counted_not_guessed() {
        let tsx = r##"<Box pos="relative">
  <Box bg="#FFF" h="22px" pos="absolute" right="36px" top="210px" w="64px" />
  <Text left="269px" pos="absolute" top="214px">x</Text>
</Box>"##;
        let report = layout_risks(tsx, None);
        assert!(report["risks"].as_array().unwrap().is_empty());
        assert_eq!(report["groupsWithoutReferenceWidth"], 1);
    }
}
