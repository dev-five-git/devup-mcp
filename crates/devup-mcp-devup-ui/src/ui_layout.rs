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
    /// Its width is the viewport's: a fluid root, or an in-flow child that a
    /// column stretches across a parent that is.
    follows_viewport: bool,
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
        ancestors: Vec::new(),
        risks: Vec::new(),
        unmeasured_groups: 0,
    };
    collector.visit_program(&parsed.program);
    json!({
        "risks": collector.risks,
        "groupsWithoutReferenceWidth": collector.unmeasured_groups,
        "checked": ["positioned siblings drawn inside a painted sibling, by the edge each is held by", "positioned boxes held by their left edge but drawn against the right edge or centred, in a container whose width follows the viewport", "straight rules drawn as a mask in a box that stretches"],
        "runtimeUnverified": ["rendered geometry", "text wrapping and line breaks", "overlap between text and shapes", "font metrics"],
    })
}

struct Collector<'s> {
    source: &'s str,
    reference_width: Option<f64>,
    next: usize,
    stack: Vec<Vec<Element>>,
    ancestors: Vec<Element>,
    risks: Vec<Value>,
    unmeasured_groups: usize,
}

impl<'a> Visit<'a> for Collector<'_> {
    fn visit_jsx_element(&mut self, element: &JSXElement<'a>) {
        let attrs = string_attributes(&element.opening_element, self.source);
        let follows_viewport = match self.ancestors.last() {
            None => fluid_width(&attrs),
            Some(parent) => parent.follows_viewport && stretches_across(parent, &attrs),
        };
        let info = Element {
            ordinal: self.next,
            tag: self.source[element.opening_element.name.span().start as usize
                ..element.opening_element.name.span().end as usize]
                .to_owned(),
            start: element.opening_element.span.start as usize,
            attrs,
            has_children: element.children.iter().any(
                |child| !matches!(child, JSXChild::Text(text) if text.value.trim().is_empty()),
            ),
            follows_viewport,
        };
        if let Some(siblings) = self.stack.last_mut() {
            siblings.push(info.clone());
        }
        self.stack.push(Vec::new());
        self.ancestors.push(info.clone());
        walk::walk_jsx_element(self, element);
        self.ancestors.pop();
        let children = self.stack.pop().unwrap_or_default();
        self.analyze(&info, &children);
    }

    fn visit_jsx_opening_element(&mut self, element: &JSXOpeningElement<'a>) {
        self.next += 1;
        walk::walk_jsx_opening_element(self, element);
    }
}

fn string_attributes(element: &JSXOpeningElement<'_>, source: &str) -> BTreeMap<String, String> {
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
                        _ => {
                            let span = container.expression.span();
                            source[span.start as usize..span.end as usize].to_owned()
                        }
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

fn narrow_value(value: &str) -> &str {
    value
        .trim()
        .strip_prefix('[')
        .unwrap_or(value)
        .split(',')
        .next()
        .unwrap_or_default()
        .trim()
        .trim_matches(['\'', '"'])
}

fn hidden_narrow(element: &Element) -> bool {
    element
        .attrs
        .get("display")
        .is_some_and(|value| narrow_value(value) == "none")
}

fn fluid_width(attrs: &BTreeMap<String, String>) -> bool {
    !attrs.contains_key("boxSize") && attrs.get("w").is_none_or(|w| w == "100%")
}

/// Whether an in-flow child takes its whole parent's width: a block `Box`, or
/// a column that stretches, with no horizontal padding on the parent and no
/// horizontal margin or own width on the child.
fn stretches_across(parent: &Element, child: &BTreeMap<String, String>) -> bool {
    let any = |attrs: &BTreeMap<String, String>, names: &[&str]| {
        names.iter().any(|name| attrs.contains_key(*name))
    };
    let column = match parent.tag.as_str() {
        "Box" => !parent
            .attrs
            .get("display")
            .is_some_and(|display| display.contains("flex") || display.contains("grid")),
        "VStack" => true,
        "Flex" => parent
            .attrs
            .get("flexDir")
            .or_else(|| parent.attrs.get("flexDirection"))
            .is_some_and(|direction| direction == "column"),
        _ => false,
    };
    column
        && parent
            .attrs
            .get("alignItems")
            .is_none_or(|align| align == "stretch")
        && !any(&parent.attrs, &["p", "px", "pl", "pr"])
        && !any(child, &["m", "mx", "ml", "mr", "maxW"])
        && child.get("pos").is_none_or(|pos| pos != "absolute")
        && fluid_width(child)
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
        fn direction(element: &Element) -> Option<&String> {
            element
                .attrs
                .get("flexDir")
                .or_else(|| element.attrs.get("flexDirection"))
        }
        let column = |element: &Element| {
            element.tag == "VStack"
                || direction(element).is_some_and(|value| value.contains("column"))
        };
        if children
            .iter()
            .filter(|child| column(child) && child.has_children)
            .count()
            >= 2
        {
            self.risks.push(json!({"rule":"column-reading-order", "confidence":"medium", "elementIndex":parent.ordinal, "byteStart":parent.start,
                "message":"Column wrappers can collapse cards in column order (01,03,02,04). Use one Grid or flex-wrap in design reading order, or explicit breakpoint order; run sequence-order at every width."}));
        }
        let row = parent.tag == "Flex"
            && direction(parent).is_none_or(|value| !value.contains("column"))
            || parent.tag == "Grid"
                && parent
                    .attrs
                    .get("gridTemplateColumns")
                    .is_some_and(|value| value.contains(" ") || value.contains("repeat"));
        let navigation = matches!(parent.tag.as_str(), "nav" | "header")
            || parent
                .attrs
                .get("as")
                .is_some_and(|tag| matches!(tag.as_str(), "nav" | "header"))
            || parent
                .attrs
                .get("role")
                .is_some_and(|role| role == "navigation")
            || parent.attrs.contains_key("data-navigation-width");
        let measured_navigation = parent
            .attrs
            .get("data-navigation-width")
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or_else(|| {
                children
                    .iter()
                    .filter_map(|child| {
                        pixels(child.attrs.get("w").or_else(|| child.attrs.get("boxSize")))
                    })
                    .sum::<f64>()
                    + pixels(parent.attrs.get("gap")).unwrap_or(0.0)
                        * children.len().saturating_sub(1) as f64
            });
        let mobile_alternative = hidden_narrow(parent)
            || parent
                .attrs
                .get("flexWrap")
                .is_some_and(|value| value.contains("wrap"))
            || direction(parent).is_some_and(|value| narrow_value(value).starts_with("column"));
        if navigation && measured_navigation > 390.0 && !mobile_alternative {
            self.risks.push(json!({"rule":"navigation-fit", "confidence":"high", "elementIndex":parent.ordinal, "byteStart":parent.start,
                "contentWidth":measured_navigation, "narrowViewport":390,
                "message":"Header navigation cannot fit a narrow container. With no mobile menu frame, propose a drawer behind a menu button below the content-fit breakpoint; verify no-clipped-content at every width."}));
        }
        let fixed_children = children
            .iter()
            .filter(|child| !hidden_narrow(child) && pixels(child.attrs.get("w")).is_some())
            .count();
        let wide_only = std::iter::once(parent).chain(children).any(|element| {
            !hidden_narrow(element)
                && element.attrs.iter().any(|(name, value)| {
                    if matches!(name.as_str(), "w" | "width" | "boxSize")
                        && value.trim_start().starts_with('[')
                        && !matches!(narrow_value(value), "null" | "undefined" | "")
                        && !narrow_value(value).ends_with("px")
                    {
                        return false;
                    }
                    (value.trim_start().starts_with("[null")
                        || value.trim_start().starts_with("[undefined"))
                        && value.contains("px")
                })
        });
        if row && !mobile_alternative && (fixed_children >= 2 || wide_only) {
            self.risks.push(json!({"rule":"narrow-fixed-columns", "confidence":"medium", "elementIndex":parent.ordinal, "byteStart":parent.start,
                "message":"A multi-column row keeps fixed child widths without a narrow column or wrap rule. Supply narrow responsive layout props and verify section-layout, overflow and clipped text at 390/858px."}));
        }
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
        let mut drawn_inside_a_sibling = Vec::new();
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
            drawn_inside_a_sibling.push(node.ordinal);
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
        if parent.follows_viewport {
            let held_left = placed
                .iter()
                .filter(|(node, place)| {
                    place.anchor == Anchor::Left
                        && place.right.is_some()
                        && !drawn_inside_a_sibling.contains(&node.ordinal)
                })
                .collect::<Vec<_>>();
            match self.reference_width {
                Some(width) => {
                    for (node, place) in held_left {
                        self.check_held_left(node, place, width);
                    }
                }
                None => unmeasured |= !held_left.is_empty(),
            }
        }
        if unmeasured {
            self.unmeasured_groups += 1;
        }
    }

    fn check_held_left(&mut self, node: &Element, place: &Placement, width: f64) {
        let (Some(left_gap), Some(right_edge)) = (place.left, place.right) else {
            return;
        };
        let right_gap = width - right_edge;
        let (rule, message) = if right_gap + EDGE_TOLERANCE < left_gap && right_gap <= width / 4.0 {
            (
                "right-aligned-but-held-left",
                format!(
                    include_str!("text/ui_layout-check_held_left-d6e0fe40.md"),
                    describe_short(node),
                    round(right_gap),
                    round(right_gap),
                ),
            )
        } else if (left_gap - right_gap).abs() <= 1.0 {
            (
                "centered-but-held-left",
                format!(
                    include_str!("text/ui_layout-check_held_left-6cb7140a.md"),
                    describe_short(node),
                ),
            )
        } else {
            return;
        };
        self.risks.push(json!({
            "rule": rule,
            "confidence": "medium",
            "referenceWidth": width,
            "elements": [self.describe(node, place)],
            "message": message,
        }));
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
                "message": include_str!("text/ui_layout-check_line_mask-ab57b482.md"),
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

fn round(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
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
        let rules = risks(&fixed, Some(360.0))
            .iter()
            .map(|risk| risk["rule"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            rules,
            ["centered-but-held-left"],
            "a fixed rule keeps its length, and only drifts off-centre in a widening box"
        );
    }

    /// A banner from a real notice screen, its root widened by hand: the
    /// decorations kept their left offsets and parted from the right edge and
    /// the centre they were drawn on (408px off at 768).
    #[test]
    fn boxes_held_left_in_a_widened_container_are_reported_by_where_they_were_drawn() {
        let widened = r##"<VStack bg="$containerBackground" pos="relative" w="100%">
  <Box bg="$primary" h="240px" overflow="hidden" pos="relative">
    <VStack bottom="20px" left="20px" pos="absolute"><Text>Notice</Text></VStack>
    <Box bg="#FFF" h="50px" left="41.57px" maskImage="url(/a.svg)" pos="absolute" top="167px" w="318.43px" />
    <Box bg="#FFF" h="46px" left="0px" maskImage="url(/b.svg)" pos="absolute" top="112px" w="360px" />
  </Box>
</VStack>"##;
        let found = risks(widened, Some(360.0));
        let rules = found
            .iter()
            .map(|risk| {
                (
                    risk["rule"].as_str().unwrap(),
                    risk["elements"][0]["elementIndex"].as_u64().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            rules,
            [
                ("right-aligned-but-held-left", 4),
                ("centered-but-held-left", 5)
            ],
            "{found:?}"
        );

        let drawn_width = widened.replace(r#"w="100%""#, r#"w="360px""#);
        assert!(
            risks(&drawn_width, Some(360.0)).is_empty(),
            "a root at its drawn width does not move"
        );

        let anchored = widened
            .replace(r#"left="41.57px" "#, r#"right="0px" "#)
            .replace(
                r#"left="0px" maskImage="url(/b.svg)""#,
                r#"left="50%" maskImage="url(/b.svg)" transform="translateX(-50%)""#,
            );
        assert!(risks(&anchored, Some(360.0)).is_empty(), "{anchored}");

        let padded = widened.replace(
            r#"<VStack bg="$containerBackground""#,
            r#"<VStack px="20px" bg="$containerBackground""#,
        );
        assert!(
            risks(&padded, Some(360.0)).is_empty(),
            "a padded parent is not the reference width"
        );

        let report = layout_risks(widened, None);
        assert!(report["risks"].as_array().unwrap().is_empty());
        assert_eq!(report["groupsWithoutReferenceWidth"], 1);
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

#[cfg(test)]
mod responsive_failure_tests {
    use super::*;
    #[test]
    fn columns_and_wide_only_fixed_rows_are_flagged() {
        let columns = r#"const A = () => <Flex flexDir={["column", "row"]}><Flex flexDir="column"><Box/><Box/></Flex><Flex flexDir="column"><Box/><Box/></Flex></Flex>"#;
        assert!(
            layout_risks(columns, None)["risks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|risk| risk["rule"] == "column-reading-order")
        );
        let fixed =
            r#"const A = () => <Flex w={[null, "1200px"]}><Box w="400px"/><Box w="400px"/></Flex>"#;
        assert!(
            crate::ui_validate::validate_devup_ui_tsx(fixed, None, true)
                .violations
                .iter()
                .any(|risk| risk.rule == "narrow-fixed-columns")
        );
        let wrapping =
            r#"const A = () => <Flex flexWrap="wrap"><Box w="400px"/><Box w="400px"/></Flex>"#;
        assert!(
            !layout_risks(wrapping, None)["risks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|risk| risk["rule"] == "narrow-fixed-columns")
        );
    }
}
