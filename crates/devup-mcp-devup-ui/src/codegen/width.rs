//! What a generated screen does when its container is not the width it was
//! drawn at, and the opt-in policy that decides it.
//!
//! A screen is drawn at one width. Its Figma constraints and auto-layout
//! sizing say how it was meant to follow a wider or narrower parent, and the
//! default export keeps only what it can prove at the drawn width: a page root
//! that places its children absolutely keeps that width in pixels, so the
//! output is exact there and nowhere else. This module reports, per node, what
//! follows a width change and what does not, and - only when the caller opts
//! in with a [`WidthPolicy`] - emits anchors that keep the drawn relationships
//! at other widths. It never infers a desktop layout the design does not hold:
//! a node whose drawn position suggests another anchor than its constraint is
//! reported with candidates for the caller to choose from.

use std::collections::BTreeMap;

use devup_mcp_figma::{RawNode, Snapshot};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::component::{CodegenOutput, Prop};
use super::layout::{Box4, format_number, placed_by_a_free_layout, px, string_prop};

/// How a positioned node is held horizontally inside its containing block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Anchor {
    Left,
    Right,
    Center,
    Stretch,
    Scale,
}

/// The caller's decision, opted into per export. With a policy the root takes
/// its container's width, and every positioned node keeps the anchor named
/// here, else the anchor of a caller-anchored container it is drawn inside,
/// else its own Figma constraint. Without one the default export is unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WidthPolicy {
    pub anchors: BTreeMap<String, Anchor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AnchorSource {
    Caller,
    Container,
    Constraint,
}

/// Tolerance for comparing drawn edges, in px. Figma geometry carries float
/// noise well below this; anything larger is a different drawing.
const EDGE_TOLERANCE: f64 = 0.5;

/// The narrowest viewport a screen is drawn for. A root drawn narrower is a
/// component its host places and sizes - a 64px loading indicator, an icon -
/// so where its parts go on a wider screen is not a question it poses.
const NARROWEST_SCREEN: f64 = 320.0;

pub(crate) fn parent_of<'a>(snapshot: &'a Snapshot, node: &RawNode) -> Option<&'a RawNode> {
    snapshot.nodes.values().find(|candidate| {
        candidate
            .typed_view()
            .child_ids()
            .any(|child| child == node.id)
    })
}

fn is_canvas_child(parent: Option<&RawNode>, node: &RawNode) -> bool {
    parent
        .map(|parent| parent.typed_view().node_type())
        .or_else(|| node.typed_view().string("parentType"))
        .is_some_and(|kind| matches!(kind, "SECTION" | "PAGE" | "COMPONENT_SET"))
}

pub(crate) fn is_positioned(snapshot: &Snapshot, node: &RawNode, parent: Option<&RawNode>) -> bool {
    node.typed_view().string("layoutPositioning") == Some("ABSOLUTE")
        || placed_by_a_free_layout(snapshot, node, parent, is_canvas_child(parent, node))
}

/// The box `push_absolute` places: the node's own, in its parent's space even
/// under a group, or the asset export it is given.
pub(crate) fn placed_box(node: &RawNode, parent: &RawNode, placed_by: Option<Box4>) -> Box4 {
    if let Some(placed) = placed_by {
        return placed;
    }
    let view = node.typed_view();
    let mut own = Box4 {
        x: view.number("x").unwrap_or(0.0),
        y: view.number("y").unwrap_or(0.0),
        w: view.number("width").unwrap_or(0.0),
        h: view.number("height").unwrap_or(0.0),
    };
    if parent.typed_view().node_type() == "GROUP" {
        match (super::layout_box(node), super::layout_box(parent)) {
            (Some(child), Some(group)) => {
                own.x = child.x - group.x;
                own.y = child.y - group.y;
            }
            _ => {
                let group = parent.typed_view();
                own.x -= group.number("x").unwrap_or(0.0);
                own.y -= group.number("y").unwrap_or(0.0);
            }
        }
    }
    own
}

fn export_box_if_asset(snapshot: &Snapshot, node: &RawNode) -> Option<Box4> {
    super::asset_kind(snapshot, node)
        .is_some()
        .then(|| super::export_box(snapshot, node))
        .flatten()
}

/// A group has no constraints of its own; like `push_absolute`, read its
/// first child's.
pub(crate) fn horizontal_constraint<'a>(snapshot: &'a Snapshot, node: &'a RawNode) -> &'a str {
    let view = node.typed_view();
    view.value("constraints")
        .and_then(Value::as_object)
        .or_else(|| {
            view.child_ids()
                .next()
                .and_then(|child| snapshot.nodes.get(child))?
                .typed_view()
                .value("constraints")
                .and_then(Value::as_object)
        })
        .and_then(|constraints| constraints.get("horizontal"))
        .and_then(Value::as_str)
        .unwrap_or("MIN")
}

fn constraint_anchor(constraint: &str) -> Anchor {
    match constraint {
        "MAX" => Anchor::Right,
        "CENTER" => Anchor::Center,
        "STRETCH" => Anchor::Stretch,
        "SCALE" => Anchor::Scale,
        _ => Anchor::Left,
    }
}

/// The smallest earlier sibling that paints a box this node is drawn inside:
/// how a flat design groups a badge's text with its background, or a card's
/// rows with the card, without a frame to say so.
fn innermost_container<'a>(
    snapshot: &'a Snapshot,
    node: &RawNode,
    parent: &'a RawNode,
) -> Option<&'a RawNode> {
    let inner = placed_box(node, parent, export_box_if_asset(snapshot, node));
    let parent_view = parent.typed_view();
    let (parent_width, parent_height) = (
        parent_view.number("width").unwrap_or(f64::INFINITY),
        parent_view.number("height").unwrap_or(f64::INFINITY),
    );
    parent
        .typed_view()
        .child_ids()
        .take_while(|id| *id != node.id)
        .filter_map(|id| snapshot.nodes.get(id))
        .filter(|candidate| {
            let view = candidate.typed_view();
            view.bool("visible") != Some(false)
                && view.node_type() != "TEXT"
                && view.child_ids().next().is_none()
                && is_positioned(snapshot, candidate, Some(parent))
        })
        .map(|candidate| {
            let outer = placed_box(candidate, parent, export_box_if_asset(snapshot, candidate));
            (candidate, outer)
        })
        .filter(|(_, outer)| {
            // A sibling covering the whole parent is the parent's own
            // background, not a group inside it.
            (outer.w + EDGE_TOLERANCE < parent_width || outer.h + EDGE_TOLERANCE < parent_height)
                && outer.x <= inner.x + EDGE_TOLERANCE
                && outer.y <= inner.y + EDGE_TOLERANCE
                && outer.x + outer.w + EDGE_TOLERANCE >= inner.x + inner.w
                && outer.y + outer.h + EDGE_TOLERANCE >= inner.y + inner.h
                && outer.w * outer.h > inner.w * inner.h
        })
        .min_by(|(_, left), (_, right)| (left.w * left.h).total_cmp(&(right.w * right.h)))
        .map(|(candidate, _)| candidate)
}

/// The anchor a positioned node keeps under `policy`, and who decided it.
pub(crate) fn effective_anchor(
    snapshot: &Snapshot,
    node: &RawNode,
    parent: &RawNode,
    policy: &WidthPolicy,
) -> (Anchor, AnchorSource) {
    if let Some(anchor) = policy.anchors.get(&node.id) {
        return (*anchor, AnchorSource::Caller);
    }
    // Only a caller-placed right or centre anchor moves a container away from
    // where its contents are pinned. A stretched or left-held container keeps
    // contents at their own anchors inside it at any width.
    if let Some(container) = innermost_container(snapshot, node, parent) {
        let (anchor, source) = effective_anchor(snapshot, container, parent, policy);
        if source != AnchorSource::Constraint && matches!(anchor, Anchor::Right | Anchor::Center) {
            return (anchor, AnchorSource::Container);
        }
    }
    (
        constraint_anchor(horizontal_constraint(snapshot, node)),
        AnchorSource::Constraint,
    )
}

/// The horizontal placement props for `anchor`, reproducing the drawn
/// position exactly at the drawn parent width.
pub(super) fn push_horizontal(
    props: &mut Vec<Prop>,
    anchor: Anchor,
    placed: Box4,
    parent_width: f64,
) {
    let right = parent_width - placed.x - placed.w;
    match anchor {
        Anchor::Left => string_prop(props, "left", px(placed.x)),
        Anchor::Right => string_prop(props, "right", px(right)),
        Anchor::Stretch => {
            string_prop(props, "left", px(placed.x));
            string_prop(props, "right", px(right));
        }
        Anchor::Scale => string_prop(props, "left", percent(placed.x, parent_width)),
        Anchor::Center => {
            let offset = placed.x + placed.w / 2.0 - parent_width / 2.0;
            let left = if format_number(offset) == "0" {
                "50%".to_owned()
            } else if offset > 0.0 {
                format!("calc(50% + {})", px(offset))
            } else {
                format!("calc(50% - {})", px(-offset))
            };
            string_prop(props, "left", left);
            string_prop(props, "transform", "translateX(-50%)");
        }
    }
}

pub(crate) fn percent(part: f64, whole: f64) -> String {
    if whole == 0.0 {
        return "0%".to_owned();
    }
    format!("{}%", format_number(part / whole * 100.0))
}

/// A vector whose bounding box has no height: every point on one horizontal
/// line, painted only by a solid stroke. The drawing is a rule of the stroke's
/// colour and weight at any length, which is the evidence that lets its mask be
/// scaled along its length without changing what it depicts.
pub(crate) fn straight_horizontal_line(node: &RawNode) -> bool {
    let view = node.typed_view();
    matches!(view.node_type(), "VECTOR" | "LINE")
        && view
            .value("absoluteBoundingBox")
            .and_then(|bounds| {
                Some((
                    bounds.get("width")?.as_f64()?,
                    bounds.get("height")?.as_f64()?,
                ))
            })
            .is_some_and(|(width, height)| width > 0.0 && height.abs() < 0.01)
        && view
            .number("rotation")
            .is_none_or(|rotation| rotation.abs() <= 0.01)
        && view
            .number("strokeWeight")
            .is_some_and(|weight| weight > 0.0)
        && view
            .value("strokes")
            .and_then(Value::as_array)
            .is_some_and(|strokes| {
                strokes.iter().any(|paint| {
                    paint.get("visible").and_then(Value::as_bool) != Some(false)
                        && paint.get("type").and_then(Value::as_str) == Some("SOLID")
                })
            })
}

/// Whether a straight line's box follows its parent's width under `policy`,
/// so its mask has to scale with the box instead of keeping its drawn length.
pub(crate) fn line_follows_width(
    snapshot: &Snapshot,
    node: &RawNode,
    policy: &WidthPolicy,
) -> bool {
    let parent = parent_of(snapshot, node);
    straight_horizontal_line(node)
        && parent.is_some_and(|parent| {
            is_positioned(snapshot, node, Some(parent))
                && matches!(
                    effective_anchor(snapshot, node, parent, policy).0,
                    Anchor::Stretch | Anchor::Scale
                )
        })
}

/// The evidence that a width policy's stretch or scale represents the node's
/// drawn width: at the drawn parent width, `P - left - right` (stretch) or
/// `share * P` (scale) equals the drawn box width. `None` when no policy
/// applies, the anchor keeps an explicit width, or the props do not resolve.
pub(crate) fn policy_width_proof(
    snapshot: &Snapshot,
    output: &CodegenOutput,
    node_id: &str,
) -> Option<Value> {
    let policy = output.width_policy.as_ref()?;
    let node = snapshot.nodes.get(node_id)?;
    let parent = parent_of(snapshot, node)?;
    if !is_positioned(snapshot, node, Some(parent)) {
        return None;
    }
    let (anchor, source) = effective_anchor(snapshot, node, parent, policy);
    let parent_width = parent.typed_view().number("width")?;
    let placed = placed_box(node, parent, export_box_if_asset(snapshot, node));
    let tag = opening(output, node_id)?;
    let pixels = |name| {
        attribute(tag, name)?
            .strip_suffix("px")?
            .parse::<f64>()
            .ok()
    };
    let resolved = match anchor {
        Anchor::Stretch if attribute(tag, "w").is_none() && attribute(tag, "boxSize").is_none() => {
            parent_width - pixels("left")? - pixels("right")?
        }
        Anchor::Scale => {
            let share = attribute(tag, "w")?
                .strip_suffix('%')?
                .parse::<f64>()
                .ok()?;
            share / 100.0 * parent_width
        }
        _ => return None,
    };
    ((resolved - placed.w).abs() < 0.01).then(|| {
        json!({
            "state": "verified",
            "fidelityImpact": "none",
            "resolution": "verified-width-policy",
            "anchor": anchor,
            "anchorSource": source,
            "sourceValue": placed.w,
            "parentWidth": parent_width,
            "resolvedPixels": resolved,
            "calculation": if anchor == Anchor::Stretch {
                "drawn parent width - left - right = drawn width"
            } else {
                "width share / 100 * drawn parent width = drawn width"
            },
            "reason": include_str!("../text/width-policy_width_proof-d2d9e821.md"),
        })
    })
}

/// The evidence that the horizontal props a width policy wrote place the node
/// at its drawn offset at the drawn parent width.
pub(crate) fn policy_horizontal_proof(
    snapshot: &Snapshot,
    output: &CodegenOutput,
    node_id: &str,
) -> Option<Value> {
    let policy = output.width_policy.as_ref()?;
    let node = snapshot.nodes.get(node_id)?;
    let parent = parent_of(snapshot, node)?;
    if !is_positioned(snapshot, node, Some(parent)) {
        return None;
    }
    let (anchor, source) = effective_anchor(snapshot, node, parent, policy);
    let parent_width = parent.typed_view().number("width")?;
    let placed = placed_box(node, parent, export_box_if_asset(snapshot, node));
    let mut expected = Vec::new();
    push_horizontal(&mut expected, anchor, placed, parent_width);
    let tag = opening(output, node_id)?;
    let matches = expected
        .iter()
        .all(|(name, super::component::PropValue::String(value))| {
            let written = attribute(tag, name);
            if name == "transform" {
                written.is_some_and(|transform| {
                    transform == "translateX(-50%)" || transform.starts_with("translate(-50%,")
                })
            } else {
                written == Some(value.as_str())
            }
        })
        && ["left", "right"]
            .iter()
            .all(|name| attribute(tag, name).is_none() || expected.iter().any(|(n, _)| n == name));
    matches.then(|| {
        json!({
            "state": "verified",
            "fidelityImpact": "none",
            "anchor": anchor,
            "anchorSource": source,
            "constraint": horizontal_constraint(snapshot, node),
            "sourceOffset": placed.x,
            "sourceSize": placed.w,
            "parentSize": parent_width,
            "generated": expected
                .iter()
                .map(|(name, super::component::PropValue::String(value))| (name.clone(), value.clone()))
                .collect::<BTreeMap<_, _>>(),
            "reason": include_str!("../text/width-policy_horizontal_proof-6cd374a8.md"),
        })
    })
}

fn opening<'a>(output: &'a CodegenOutput, node_id: &str) -> Option<&'a str> {
    crate::provenance::node_opening(output, node_id)
}

fn attribute<'a>(opening: &'a str, name: &str) -> Option<&'a str> {
    crate::provenance::opening_prop(opening, name)
}

/// The width-change report for one generated screen.
///
/// Static, from the collected constraints, auto-layout sizing and drawn
/// geometry beside the generated props; nothing here is rendered. `exact` in
/// the export's quality is a statement about the drawn width, and this says
/// what that statement does not cover.
pub fn width_behavior(snapshot: &Snapshot, root_id: &str, output: &CodegenOutput) -> Value {
    const MAX_FINDINGS: usize = 40;
    let Some(mut root) = snapshot.nodes.get(root_id) else {
        return Value::Null;
    };
    if root.typed_view().node_type() == "SECTION"
        && let Some(first) = root
            .typed_view()
            .child_ids()
            .next()
            .and_then(|id| snapshot.nodes.get(id))
    {
        root = first;
    }
    let policy = output.width_policy.as_ref();
    let root_opening = opening(output, &root.id).unwrap_or("");
    let root_width = attribute(root_opening, "w").or_else(|| attribute(root_opening, "boxSize"));
    let root_fluid = root_width.is_none_or(|width| !width.ends_with("px"));
    let design_width = root.typed_view().number("width");
    let component_sized = design_width.is_some_and(|width| width < NARROWEST_SCREEN);

    let mut findings: Vec<Value> = Vec::new();
    let mut summary = BTreeMap::<&str, usize>::new();
    let mut omitted_shapes = Vec::new();
    // Depth-first with the answer to "does this node's width move when the
    // root's does?" carried down: the root's by hypothesis, a child's only
    // through FILL in flow or a stretching anchor.
    let mut pending = vec![(root, true)];
    while let Some((node, width_moves)) = pending.pop() {
        let children = node
            .typed_view()
            .child_ids()
            .filter_map(|id| snapshot.nodes.get(id))
            .filter(|child| child.typed_view().bool("visible") != Some(false))
            .collect::<Vec<_>>();
        // Index into `findings` of each sibling reported as drawn against the
        // right edge, so what is drawn inside it is folded into its finding:
        // anchoring that container carries its contents with it.
        let mut right_held = BTreeMap::<&str, usize>::new();
        let mut moves = Vec::with_capacity(children.len());
        for child in &children {
            let view = child.typed_view();
            let positioned = is_positioned(snapshot, child, Some(node));
            let child_opening = opening(output, &child.id).unwrap_or("");
            let finding = if positioned {
                *summary.entry("positioned").or_default() += 1;
                if view.child_ids().next().is_none()
                    && view.node_type() != "TEXT"
                    && super::asset_kind(snapshot, child).is_none()
                    && attribute(child_opening, "w").is_none()
                    && attribute(child_opening, "boxSize").is_none()
                    && attribute(child_opening, "h") == Some("100%")
                {
                    omitted_shapes.push(child.id.clone());
                }
                positioned_finding(snapshot, child, node, width_moves, child_opening, policy)
            } else {
                *summary.entry("inFlow").or_default() += 1;
                in_flow_finding(snapshot, child, node, width_moves, child_opening)
            };
            moves.push(
                width_moves
                    && if positioned {
                        let anchor = policy.map_or_else(
                            || constraint_anchor(horizontal_constraint(snapshot, child)),
                            |policy| effective_anchor(snapshot, child, node, policy).0,
                        );
                        matches!(anchor, Anchor::Stretch | Anchor::Scale)
                            && attribute(child_opening, "w").is_none_or(|w| !w.ends_with("px"))
                    } else {
                        view.string("layoutSizingHorizontal") == Some("FILL")
                    },
            );
            let Some(finding) = finding else {
                continue;
            };
            let carrier = finding["drawnInside"]
                .as_str()
                .and_then(|container| right_held.get(container).copied());
            if finding["kind"] == "right-aligned-but-held-left"
                && let Some(index) = carrier
            {
                findings[index]["carries"]
                    .as_array_mut()
                    .expect("right-held findings start with carries")
                    .push(json!(child.id));
                continue;
            }
            let status = finding["status"].as_str().unwrap_or("").to_owned();
            if component_sized && status == "consumer-policy-required" {
                *summary.entry("componentRootSuppressed").or_default() += 1;
                continue;
            }
            *summary.entry(status_key(&status)).or_default() += 1;
            if findings.len() >= MAX_FINDINGS {
                *summary.entry("truncatedFindings").or_default() += 1;
                continue;
            }
            if finding["kind"] == "right-aligned-but-held-left" {
                right_held.insert(&child.id, findings.len());
            }
            findings.push(finding);
        }
        for (child, child_moves) in children.into_iter().zip(moves).rev() {
            pending.push((child, child_moves));
        }
    }
    summary.insert("shapeSizeOmitted", omitted_shapes.len());
    let mut report = json!({
        "designWidth": design_width,
        "rootNodeId": root.id,
        "root": {
            "generatedWidth": root_width,
            "fluid": root_fluid,
            "componentSized": component_sized,
            "note": if component_sized {
                include_str!("../text/width-width_behavior-fdf7cb4e.md")
            } else if root_fluid {
                "The root takes its container's width, so every finding below applies at any viewport other than the drawn width."
            } else {
                include_str!("../text/width-width_behavior-8f8e0a0f.md")
            },
        },
        "exactnessScope": "quality.projection grades the mapping at the drawn width only. It is not a claim about other viewport widths.",
        "verification": {
            "static": "Figma constraints, auto-layout sizing and drawn geometry compared with the generated props.",
            "runtime": "not-performed",
            "runtimeUnverified": ["text wrapping and line breaks", "overlap between text and shapes", "content escaping its card", "font metrics"],
        },
        "summary": summary,
        "findings": findings,
    });
    if !omitted_shapes.is_empty() {
        omitted_shapes.truncate(10);
        report["shapeSizeOmitted"] = json!({
            "nodeIds": omitted_shapes,
            "note": include_str!("../text/width-width_behavior-d9663795.md"),
        });
    }
    if let Some(policy) = policy {
        report["widthPolicy"] = json!({"applied": true, "callerAnchors": policy.anchors});
    }
    report
}

fn status_key(status: &str) -> &'static str {
    match status {
        "generator-approximation" => "generatorApproximation",
        "consumer-policy-required" => "consumerPolicyRequired",
        "design-defined" => "designDefined",
        _ => "other",
    }
}

fn positioned_finding(
    snapshot: &Snapshot,
    node: &RawNode,
    parent: &RawNode,
    parent_width_moves: bool,
    generated: &str,
    policy: Option<&WidthPolicy>,
) -> Option<Value> {
    if !parent_width_moves {
        return None;
    }
    let parent_width = parent.typed_view().number("width")?;
    let placed = placed_box(node, parent, export_box_if_asset(snapshot, node));
    let left_gap = placed.x;
    let right_gap = parent_width - placed.x - placed.w;
    let constraint = horizontal_constraint(snapshot, node);
    let declared = node.typed_view().value("constraints").is_some();
    let (anchor, source) = policy.map_or(
        (constraint_anchor(constraint), AnchorSource::Constraint),
        |policy| effective_anchor(snapshot, node, parent, policy),
    );
    let generated_props = ["left", "right", "w", "transform", "maskSize"]
        .into_iter()
        .filter_map(|name| attribute(generated, name).map(|value| (name, value)))
        .collect::<BTreeMap<_, _>>();
    let evidence = json!({
        "constraint": constraint,
        "constraintDeclared": declared,
        "parentId": parent.id,
        "parentWidth": parent_width,
        "x": placed.x,
        "width": placed.w,
        "leftGap": left_gap,
        "rightGap": right_gap,
        "anchor": anchor,
        "anchorSource": source,
        "generated": generated_props,
    });
    let line = straight_horizontal_line(node) && super::asset_kind(snapshot, node).is_some();
    let container = innermost_container(snapshot, node, parent);
    let container_anchor = container.map(|container| {
        policy.map_or(
            constraint_anchor(horizontal_constraint(snapshot, container)),
            |policy| effective_anchor(snapshot, container, parent, policy).0,
        )
    });
    let finding = |kind: &str, status: &str, confidence: &str, note: &str, candidates: Value| {
        let mut finding = json!({
            "nodeId": node.id,
            "name": node.typed_view().name(),
            "kind": kind,
            "status": status,
            "confidence": confidence,
            "evidence": evidence,
            "note": note,
        });
        if !candidates.is_null() {
            finding["candidates"] = candidates;
        }
        if let Some(container) = container {
            finding["drawnInside"] = json!(container.id);
        }
        if line {
            finding["straightLineMask"] = json!(true);
        }
        finding
    };
    if line
        && matches!(anchor, Anchor::Stretch | Anchor::Scale)
        && generated_props.get("maskSize") != Some(&"100% 100%")
    {
        return Some(finding(
            "fixed-length-line-mask",
            "generator-approximation",
            "high",
            "A straight line is drawn as a mask that keeps its drawn length while its box follows the parent's width.",
            Value::Null,
        ));
    }
    if let (Some(container_anchor), Some(container)) = (container_anchor, container)
        && separates(container_anchor, anchor)
    {
        return Some(finding(
            "group-anchor-mismatch",
            "consumer-policy-required",
            "high",
            &format!(
                "Drawn inside {} but held {} while it is held {}; the two part at any other width. Anchor the container in widthPolicy and its contents follow it.",
                container.id,
                anchor_name(anchor),
                anchor_name(container_anchor)
            ),
            Value::Null,
        ));
    }
    if source == AnchorSource::Constraint
        && matches!(constraint, "STRETCH" | "SCALE")
        && policy.is_none()
    {
        return Some(finding(
            "constraint-not-expressed",
            "generator-approximation",
            "high",
            "The Figma constraint follows the parent's width, and the default export writes the drawn offset and width instead. widthPolicy writes the constraint.",
            Value::Null,
        ));
    }
    if constraint == "CENTER"
        && policy.is_none()
        && (placed.x + placed.w / 2.0 - parent_width / 2.0).abs() >= EDGE_TOLERANCE
    {
        return Some(finding(
            "center-offset-dropped",
            "generator-approximation",
            "high",
            "Centred by constraint but drawn off-centre; the default export centres it exactly, so it moves by its offset even at the drawn width. widthPolicy keeps the offset.",
            Value::Null,
        ));
    }
    let already_follows =
        [attribute(generated, "w"), attribute(generated, "boxSize")].contains(&Some("100%"));
    if anchor != Anchor::Left || source != AnchorSource::Constraint || already_follows {
        return None;
    }
    if right_gap + EDGE_TOLERANCE < left_gap && right_gap <= parent_width / 4.0 {
        let mut found = finding(
            "right-aligned-but-held-left",
            "consumer-policy-required",
            "medium",
            include_str!("../text/width-positioned_finding-7a9e6965.md"),
            json!([
                {"anchor": "left", "source": "design-constraint"},
                {"anchor": "right", "rightPx": round(right_gap)},
            ]),
        );
        found["carries"] = json!([]);
        return Some(found);
    }
    if (left_gap - right_gap).abs() <= 1.0 {
        return Some(finding(
            "centered-but-held-left",
            "consumer-policy-required",
            "medium",
            "Drawn with equal gaps on both sides, but its constraint holds it at its left offset. Whether it should stay, centre, or stretch between those gaps is not in the design.",
            json!([
                {"anchor": "left", "source": "design-constraint"},
                {"anchor": "center"},
                {"anchor": "stretch", "leftPx": round(left_gap), "rightPx": round(right_gap)},
            ]),
        ));
    }
    None
}

fn in_flow_finding(
    snapshot: &Snapshot,
    node: &RawNode,
    parent: &RawNode,
    parent_width_moves: bool,
    generated: &str,
) -> Option<Value> {
    if !parent_width_moves {
        return None;
    }
    let view = node.typed_view();
    let parent_width = parent.typed_view().number("width")?;
    let width = view.number("width")?;
    let sizing = view.string("layoutSizingHorizontal");
    let generated_props = ["w", "flex", "maskSize"]
        .into_iter()
        .filter_map(|name| attribute(generated, name).map(|value| (name, value)))
        .collect::<BTreeMap<_, _>>();
    let evidence = json!({
        "sizing": sizing,
        "parentId": parent.id,
        "parentWidth": parent_width,
        "width": width,
        "generated": generated_props,
    });
    let line = straight_horizontal_line(node) && super::asset_kind(snapshot, node).is_some();
    if line && sizing == Some("FILL") && attribute(generated, "maskSize") != Some("100% 100%") {
        return Some(json!({
            "nodeId": node.id, "name": view.name(), "kind": "fixed-length-line-mask",
            "status": "generator-approximation", "confidence": "high", "evidence": evidence,
            "straightLineMask": true,
            "note": "A straight line FILLs its parent's width, and its mask keeps its drawn length inside the wider box.",
        }));
    }
    (sizing == Some("FIXED") && view.node_type() != "TEXT" && width * 2.0 >= parent_width).then(|| {
        json!({
            "nodeId": node.id, "name": view.name(), "kind": "fixed-width-in-fluid-parent",
            "status": "design-defined", "confidence": "high", "evidence": evidence,
            "note": "FIXED in the design, so it keeps its drawn width while its parent follows the viewport. Stretching it is a layout decision the design does not make.",
        })
    })
}

/// Whether a container and the node drawn inside it part company when the
/// parent's width changes.
fn separates(container: Anchor, node: Anchor) -> bool {
    container != node && container != Anchor::Stretch
}

fn anchor_name(anchor: Anchor) -> &'static str {
    match anchor {
        Anchor::Left => "left",
        Anchor::Right => "right",
        Anchor::Center => "centred",
        Anchor::Stretch => "stretched",
        Anchor::Scale => "scaled",
    }
}

fn round(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}
