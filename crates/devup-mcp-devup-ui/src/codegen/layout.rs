use devup_mcp_figma::{RawNode, Snapshot};
use serde_json::Value;

use super::component::{Prop, PropValue, RootLayout};

pub(super) fn push_layout_props(
    snapshot: &Snapshot,
    node: &RawNode,
    component: &str,
    props: &mut Vec<Prop>,
    root_layout: RootLayout,
    is_render_root: bool,
) {
    let view = node.typed_view();
    let parent = snapshot.nodes.values().find(|candidate| {
        candidate
            .typed_view()
            .child_ids()
            .any(|child| child == node.id)
    });
    let is_root = snapshot.roots.iter().any(|root| root == &node.id);
    // The parent of a collected root sits outside the collected subtree, so it
    // cannot be looked up and the node's recorded parent type is the only
    // account of it. Without that fallback a screen read as having no parent at
    // all and its canvas width was emitted as a real constraint, pinning the
    // result to a device size that does not exist.
    let is_page_root = parent
        .map(|parent| parent.typed_view().node_type())
        .or_else(|| view.string("parentType"))
        .is_some_and(|kind| matches!(kind, "SECTION" | "PAGE" | "COMPONENT_SET"));
    let fixed_w = view.string("layoutSizingHorizontal") == Some("FIXED");
    let fixed_h = view.string("layoutSizingVertical") == Some("FIXED");
    let fill_w = view.string("layoutSizingHorizontal") == Some("FILL");
    let fill_h = view.string("layoutSizingVertical") == Some("FILL");
    let absolute = view.string("layoutPositioning") == Some("ABSOLUTE");
    let embedded_root = is_render_root && root_layout == RootLayout::Embedded;
    let mut width = None;
    let mut height = None;

    if embedded_root {
        // The selected frame is being inserted into an existing page layout.
        // Preserve its visual/layout semantics, but do not constrain the host
        // with Figma canvas geometry or root positioning.
    } else if absolute {
        push_absolute(node, parent, props);
        if matches!(component, "Image" | "Text") {
            width = view.number("width").map(px);
            height = Some("100%".to_owned());
        } else if view.child_ids().next().is_some() {
            width = match (
                view.number("width"),
                parent.and_then(|parent| parent.typed_view().number("width")),
            ) {
                (Some(width), Some(parent_width)) if width >= parent_width => Some("100%".into()),
                _ => None,
            };
            height = None;
        } else if view.node_type() == "FRAME"
            && let Some(parent) = parent
        {
            width = match (view.number("width"), parent.typed_view().number("width")) {
                (Some(width), Some(parent_width)) if width == parent_width => Some("100%".into()),
                (Some(width), _) => Some(px(width)),
                _ => None,
            };
            height = match (view.number("height"), parent.typed_view().number("height")) {
                (Some(height), Some(parent_height)) if height == parent_height => {
                    Some("100%".into())
                }
                (Some(height), _) => Some(px(height)),
                _ => None,
            };
        } else if let Some(parent) = parent {
            width = match (view.number("width"), parent.typed_view().number("width")) {
                (Some(width), Some(parent_width)) if width == parent_width => Some("100%".into()),
                _ => None,
            };
            height = Some("100%".to_owned());
        }
        // An absolutely positioned node is out of flow, so nothing constrains
        // it from the outside and the branches above may leave it sizeless,
        // expecting its children to define the box. That is wrong whenever
        // Figma pinned the size: a folded asset has no children left to
        // measure, and a container whose children are smaller than the frame
        // shrinks to the wrong size. Restate what Figma fixed.
        if fixed_w && fixed_h && width.is_none() && height.is_none() {
            width = view.number("width").map(px);
            height = view.number("height").map(px);
        }
    } else if is_page_root {
        // Figma page roots define the component canvas; their editor dimensions
        // are not emitted as runtime constraints.
    } else if fixed_w || fixed_h {
        if fixed_w {
            width = view.number("width").map(px);
        }
        if fixed_h {
            height = view.number("height").map(px);
        }
        if fill_w
            && (view.value("maxWidth") != Some(&Value::Null)
                || parent.is_some_and(|parent| child_shrinker(parent, "width")))
        {
            width = Some("100%".to_owned());
        }
        if fill_h
            && (view.value("maxHeight") != Some(&Value::Null)
                || parent.is_some_and(|parent| child_shrinker(parent, "height")))
        {
            height = Some("100%".to_owned());
        }
    } else if is_root {
        let no_dimensions = view.number("width").is_none() && view.number("height").is_none();
        let has_children = view.child_ids().next().is_some();
        let implicit_text_fill = view.node_type() == "TEXT"
            && view.value("layoutSizingHorizontal").is_none()
            && view.value("layoutSizingVertical").is_none();
        let standalone_component = view.node_type() == "COMPONENT"
            && !snapshot
                .nodes
                .values()
                .any(|node| node.typed_view().node_type() == "COMPONENT_SET");
        if (no_dimensions && (view.node_type() != "COMPONENT" || standalone_component))
            || has_children
            || fill_w
            || fill_h
            || view.node_type() == "GROUP"
            || implicit_text_fill
        {
            width = Some("100%".to_owned());
            height = Some("100%".to_owned());
        }
    } else {
        if fill_w
            && (view.value("maxWidth") != Some(&Value::Null)
                || parent.is_some_and(|parent| child_shrinker(parent, "width")))
        {
            width = Some("100%".to_owned());
        } else if fixed_w {
            width = view.number("width").map(px);
        }
        if fill_h
            && (view.value("maxHeight") != Some(&Value::Null)
                || parent.is_some_and(|parent| child_shrinker(parent, "height")))
        {
            height = Some("100%".to_owned());
        } else if fixed_h {
            height = view.number("height").map(px);
        }
        if view.node_type() != "COMPONENT"
            && view.number("width").is_none()
            && view.number("height").is_none()
        {
            width = Some("100%".to_owned());
            height = Some("100%".to_owned());
        }
    }

    if component == "Text" && fixed_w && fixed_h {
        match view.string("textAutoResize") {
            Some("WIDTH_AND_HEIGHT") => {
                if view.number("width").is_some() || view.number("height").is_some() {
                    width = None;
                    height = None;
                }
            }
            Some("HEIGHT") => {
                if let Some(text_width) = view.number("width") {
                    width = Some(px(text_width));
                    height = None;
                }
            }
            Some("NONE" | "TRUNCATE") if !is_page_root => {
                width = view.number("width").map(px).or(width);
                height = view.number("height").map(px).or(height);
            }
            _ => {}
        }
    }

    if let (Some(width), Some(height)) = (&width, &height)
        && width == height
    {
        string_prop(props, "boxSize", width.clone());
    } else {
        if let Some(width) = width {
            string_prop(props, "w", width);
        }
        if let Some(height) = height {
            string_prop(props, "h", height);
        }
    }

    if let Some(aspect) = view.value("targetAspectRatio").and_then(Value::as_object)
        && let (Some(x), Some(y)) = (
            aspect.get("x").and_then(Value::as_f64),
            aspect.get("y").and_then(Value::as_f64),
        )
        && y != 0.0
    {
        string_prop(
            props,
            "aspectRatio",
            format_number((x / y * 100.0).floor() / 100.0),
        );
    }
    for (field, prop) in [
        ("maxWidth", "maxW"),
        ("maxHeight", "maxH"),
        ("minWidth", "minW"),
        ("minHeight", "minH"),
    ] {
        if let Some(value) = view.number(field) {
            string_prop(props, prop, px(value));
        }
    }
    if view.string("parentId").is_some()
        && let Some(parent) = parent
        && parent
            .typed_view()
            .value("inferredAutoLayout")
            .and_then(Value::as_object)
            .and_then(|layout| layout.get("layoutMode"))
            .and_then(Value::as_str)
            == Some("GRID")
    {
        let column = view.number("gridColumnAnchorIndex").unwrap_or(-1.0);
        let row = view.number("gridRowAnchorIndex").unwrap_or(-1.0);
        let column_count = parent.typed_view().number("gridColumnCount").unwrap_or(0.0);
        let current = column + row * column_count;
        let natural = parent
            .typed_view()
            .child_ids()
            .position(|child| child == node.id)
            .map(|index| index as f64);
        if column >= 0.0 && row >= 0.0 && natural != Some(current) {
            string_prop(
                props,
                "gridColumn",
                format!("{} / span 1", format_number(column + 1.0)),
            );
            string_prop(
                props,
                "gridRow",
                format!("{} / span 1", format_number(row + 1.0)),
            );
        }
    }
    if fill_w
        && parent
            .is_some_and(|parent| parent.typed_view().string("layoutMode") == Some("HORIZONTAL"))
    {
        string_prop(props, "flex", "1");
    }

    push_auto_layout(snapshot, node, component, props);
    push_padding(snapshot, node, props);
    if view.bool("clipsContent") == Some(true) {
        string_prop(props, "overflow", "hidden");
    }
    // An absolutely positioned child needs a positioned ancestor to resolve
    // against — but a node folded into a single asset has no children left in
    // the output, so there is nothing to anchor and the containing block would
    // exist for no one.
    if !embedded_root
        && !is_page_root
        && super::style::asset_kind(snapshot, node).is_none()
        && view.child_ids().any(|child| {
            snapshot.nodes.get(child).is_some_and(|child| {
                child.typed_view().string("layoutPositioning") == Some("ABSOLUTE")
            })
        })
    {
        string_prop(props, "pos", "relative");
    }
    if let Some(rotation) = view.number("rotation")
        && rotation.abs() > 0.01
    {
        string_prop(
            props,
            "transform",
            format!("rotate({}deg)", format_number(-rotation)),
        );
        if absolute {
            string_prop(props, "transformOrigin", "top left");
        }
    }
}

pub(super) fn absolute_layout_is_exact(snapshot: &Snapshot, node: &RawNode) -> bool {
    let view = node.typed_view();
    if view.string("layoutPositioning") != Some("ABSOLUTE") {
        return true;
    }
    let parent = snapshot.nodes.values().find(|candidate| {
        candidate
            .typed_view()
            .child_ids()
            .any(|child_id| child_id == node.id)
    });
    let has_geometry = ["x", "y", "width", "height"]
        .into_iter()
        .all(|field| view.number(field).is_some_and(f64::is_finite));
    let constraints = view.value("constraints").and_then(Value::as_object);
    let supported_constraint = |axis: &str| {
        constraints
            .and_then(|value| value.get(axis))
            .and_then(Value::as_str)
            .is_none_or(|value| matches!(value, "MIN" | "MAX"))
    };
    let no_rotation = view.number("rotation").is_none_or(|value| value == 0.0);
    let exact_size = parent.is_some_and(|parent| {
        // A node pinned on both axes now emits those exact dimensions even
        // when it has children, because the absolute branch of
        // `push_layout_props` restates them rather than letting the children
        // define the box. Keep this in step with that branch: judging such a
        // node approximated would report a loss the output no longer has.
        if view.string("layoutSizingHorizontal") == Some("FIXED")
            && view.string("layoutSizingVertical") == Some("FIXED")
            && view.child_ids().next().is_some()
        {
            return true;
        }
        if view.node_type() != "FRAME" {
            return false;
        }
        if view.child_ids().next().is_none() {
            return true;
        }
        let inferred_auto_layout = view
            .value("inferredAutoLayout")
            .and_then(Value::as_object)
            .is_some();
        let parent = parent.typed_view();
        let horizontal = view.string("layoutSizingHorizontal") == Some("HUG")
            && inferred_auto_layout
            || matches!(
                (view.number("width"), parent.number("width")),
                (Some(width), Some(parent_width)) if width == parent_width
            );
        let vertical = view.string("layoutSizingVertical") == Some("HUG") && inferred_auto_layout;
        horizontal && vertical
    });
    parent.is_some()
        && has_geometry
        && supported_constraint("horizontal")
        && supported_constraint("vertical")
        && no_rotation
        && exact_size
}

fn child_shrinker(parent: &RawNode, dimension: &str) -> bool {
    let inferred = parent
        .typed_view()
        .value("inferredAutoLayout")
        .and_then(Value::as_object);
    match dimension {
        "width" => inferred.is_some_and(|layout| {
            layout.get("layoutMode").and_then(Value::as_str) == Some("VERTICAL")
                && layout.get("counterAxisAlignItems").and_then(Value::as_str) == Some("CENTER")
        }),
        "height" => inferred.is_some_and(|layout| {
            layout.get("layoutMode").and_then(Value::as_str) == Some("HORIZONTAL")
                && layout.get("counterAxisAlignItems").and_then(Value::as_str) == Some("CENTER")
        }),
        _ => false,
    }
}

fn push_auto_layout(snapshot: &Snapshot, node: &RawNode, component: &str, props: &mut Vec<Prop>) {
    let view = node.typed_view();
    let Some(layout) = view.value("inferredAutoLayout").and_then(Value::as_object) else {
        return;
    };
    let mode = layout.get("layoutMode").and_then(Value::as_str);
    if !matches!(mode, Some("HORIZONTAL" | "VERTICAL" | "GRID")) {
        return;
    }
    if mode == Some("GRID") {
        string_prop(
            props,
            "gridTemplateColumns",
            format!(
                "repeat({}, 1fr)",
                format_number(view.number("gridColumnCount").unwrap_or(0.0))
            ),
        );
        string_prop(
            props,
            "gridTemplateRows",
            format!(
                "repeat({}, 1fr)",
                format_number(view.number("gridRowCount").unwrap_or(0.0))
            ),
        );
        let row = view.number("gridRowGap").unwrap_or(0.0);
        let column = view.number("gridColumnGap").unwrap_or(0.0);
        if row == column {
            if row != 0.0 {
                string_prop(props, "gap", px(row));
            }
        } else {
            string_prop(props, "rowGap", px(row));
            string_prop(props, "columnGap", px(column));
        }
        return;
    }
    let justify = match view.string("primaryAxisAlignItems") {
        Some("MIN") => None,
        Some("MAX") => Some("flex-end"),
        Some("CENTER") => Some("center"),
        Some("SPACE_BETWEEN") => Some("space-between"),
        _ => None,
    };
    let align = match view.string("counterAxisAlignItems") {
        Some("MIN") => None,
        Some("MAX") => Some("flex-end"),
        Some("CENTER") => Some("center"),
        Some("BASELINE") => Some("baseline"),
        _ => None,
    };
    if component != "Center" {
        if let Some(value) = justify {
            string_prop(props, "justifyContent", value);
        }
        if let Some(value) = align {
            string_prop(props, "alignItems", value);
        }
    }
    if component == "Center" && mode == Some("VERTICAL") {
        string_prop(props, "flexDir", "column");
    }
    // Spacing only means something between things that are actually there. A
    // hidden child is not rendered, so a frame holding one visible child and
    // one `display: none` sibling has nothing to space apart, and naming a gap
    // implies a separation the design does not have.
    let visible_children = view
        .child_ids()
        .filter_map(|id| snapshot.nodes.get(id))
        .filter(|child| child.typed_view().bool("visible") != Some(false))
        .count();
    if visible_children > 1 && view.string("primaryAxisAlignItems") != Some("SPACE_BETWEEN") {
        let gap = layout
            .get("itemSpacing")
            .and_then(Value::as_f64)
            .or_else(|| view.number("itemSpacing"));
        if let Some(gap) = gap.filter(|gap| *gap != 0.0) {
            string_prop(props, "gap", px(gap));
        }
    }
}

/// The gap between a frame's edges and the box its children occupy.
///
/// Figma reports this as the padding of the auto-layout it infers for a frame
/// that has none. When it declines to infer one the same quantity still
/// describes the frame, so measure it rather than fall back to the frame's own
/// padding fields, which linger from whenever it last had a layout and no
/// longer place anything.
pub(super) fn children_inset(snapshot: &Snapshot, node: &RawNode) -> Option<[f64; 4]> {
    let view = node.typed_view();
    let (width, height) = (view.number("width")?, view.number("height")?);
    let mut bounds: Option<[f64; 4]> = None;
    for child in view.child_ids().filter_map(|id| snapshot.nodes.get(id)) {
        let child = child.typed_view();
        if child.bool("visible") == Some(false) {
            continue;
        }
        let (Some(x), Some(y), Some(child_width), Some(child_height)) = (
            child.number("x"),
            child.number("y"),
            child.number("width"),
            child.number("height"),
        ) else {
            continue;
        };
        bounds = Some(match bounds {
            Some([left, top, right, bottom]) => [
                left.min(x),
                top.min(y),
                right.max(x + child_width),
                bottom.max(y + child_height),
            ],
            None => [x, y, x + child_width, y + child_height],
        });
    }
    let [left, top, right, bottom] = bounds?;
    let inset = [top, width - right, height - bottom, left];
    // Children can sit outside the frame, and a negative padding describes
    // nothing.
    inset.iter().all(|edge| *edge >= 0.0).then_some(inset)
}

fn push_padding(snapshot: &Snapshot, node: &RawNode, props: &mut Vec<Prop>) {
    let view = node.typed_view();
    let inferred = view.value("inferredAutoLayout").and_then(Value::as_object);
    let derived = (inferred.is_none() && view.string("layoutMode") == Some("NONE"))
        .then(|| children_inset(snapshot, node))
        .flatten();
    let get = |name: &str| {
        inferred
            .and_then(|value| value.get(name))
            .and_then(Value::as_f64)
            .or_else(|| {
                derived.map(|[top, right, bottom, left]| match name {
                    "paddingTop" => top,
                    "paddingRight" => right,
                    "paddingBottom" => bottom,
                    _ => left,
                })
            })
            .or_else(|| view.number(name))
    };
    let [Some(top), Some(right), Some(bottom), Some(left)] = [
        get("paddingTop"),
        get("paddingRight"),
        get("paddingBottom"),
        get("paddingLeft"),
    ] else {
        return;
    };
    if top == 0.0 && right == 0.0 && bottom == 0.0 && left == 0.0 {
        return;
    }
    // A zero padding is the default, so naming it says nothing. Emitting it
    // only because the other axis happened to be padded left props like
    // `px="0px"` sitting next to a real `py`.
    let mut push = |name: &str, value: f64| {
        if value != 0.0 {
            string_prop(props, name, px(value));
        }
    };
    // Compare the values as they will be written. Insets measured from a
    // child's position carry the arithmetic's noise — a 20px box around a
    // 14.285714px child gives 2.857142686 on one side and 2.857143163 on the
    // other — and those are the same padding to anyone reading the result.
    // Comparing the raw floats split it into four separate sides.
    let same = |left: f64, right: f64| px(left) == px(right);
    if same(top, right) && same(right, bottom) && same(bottom, left) {
        push("p", top);
    } else {
        if same(top, bottom) {
            push("py", top);
        } else {
            push("pt", top);
            push("pb", bottom);
        }
        if same(left, right) {
            push("px", left);
        } else {
            push("pl", left);
            push("pr", right);
        }
    }
}

fn push_absolute(node: &RawNode, parent: Option<&RawNode>, props: &mut Vec<Prop>) {
    string_prop(props, "pos", "absolute");
    let view = node.typed_view();
    let Some(parent) = parent else {
        return;
    };
    let parent = parent.typed_view();
    let constraints = view.value("constraints").and_then(Value::as_object);
    let horizontal = constraints
        .and_then(|value| value.get("horizontal"))
        .and_then(Value::as_str)
        .unwrap_or("MIN");
    let vertical = constraints
        .and_then(|value| value.get("vertical"))
        .and_then(Value::as_str)
        .unwrap_or("MIN");
    let x = view.number("x").unwrap_or(0.0);
    let y = view.number("y").unwrap_or(0.0);
    match horizontal {
        "MAX" => string_prop(
            props,
            "right",
            px(parent.number("width").unwrap_or(0.0) - x - view.number("width").unwrap_or(0.0)),
        ),
        "CENTER" => {
            string_prop(props, "left", "50%");
            string_prop(props, "transform", "translateX(-50%)");
        }
        _ => string_prop(props, "left", px(x)),
    }
    match vertical {
        "MAX" => string_prop(
            props,
            "bottom",
            px(parent.number("height").unwrap_or(0.0) - y - view.number("height").unwrap_or(0.0)),
        ),
        "CENTER" => {
            string_prop(props, "top", "50%");
            if let Some((_, PropValue::String(value))) =
                props.iter_mut().find(|(name, _)| name == "transform")
            {
                *value = "translate(-50%, -50%)".into();
            } else {
                string_prop(props, "transform", "translateY(-50%)");
            }
        }
        _ => string_prop(props, "top", px(y)),
    }
}

pub(super) fn string_prop(props: &mut Vec<Prop>, name: &str, value: impl Into<String>) {
    let value = PropValue::String(value.into());
    if let Some((_, existing)) = props.iter_mut().find(|(existing, _)| existing == name) {
        *existing = value;
    } else {
        props.push((name.to_owned(), value));
    }
}

pub(super) fn px(value: f64) -> String {
    format!("{}px", format_number(value))
}

pub(super) fn format_number(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded == 0.0 {
        "0".to_owned()
    } else if rounded.fract() == 0.0 {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.2}").trim_end_matches('0').to_owned()
    }
}
