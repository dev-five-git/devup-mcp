use devup_mcp_figma::{RawNode, Snapshot, TypedNode};
use serde_json::Value;

use super::component::{Prop, PropValue, RootLayout};
use super::width::{Anchor, WidthPolicy};

/// A box in a parent's coordinates: left, top, width, height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Box4 {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

fn box4(value: Option<&Value>) -> Option<Box4> {
    let value = value?;
    Some(Box4 {
        x: value.get("x")?.as_f64()?,
        y: value.get("y")?.as_f64()?,
        w: value.get("width")?.as_f64()?,
        h: value.get("height")?.as_f64()?,
    })
}

/// What Figma exports for an asset node, in its parent's coordinates: the
/// node's render bounds, which frame its SVG and PNG exports - measured
/// against `exportAsync` on the official server, 2026-09-07: an instance of
/// 1373x98 whose vector sits inside it exports as 952x104, and a group of
/// 686x735 rotated four degrees exports as 759x585 with the rotation drawn
/// into the paths. `None` where the snapshot does not carry the bounds.
pub(crate) fn export_box(snapshot: &Snapshot, node: &RawNode) -> Option<Box4> {
    let view = node.typed_view();
    let render = box4(view.value("absoluteRenderBounds"))?;
    let parent = view
        .string("parentId")
        .and_then(|parent_id| snapshot.nodes.get(parent_id))?;
    let parent_box = box4(parent.typed_view().value("absoluteBoundingBox"))?;
    Some(Box4 {
        x: render.x - parent_box.x,
        y: render.y - parent_box.y,
        w: render.w,
        h: render.h,
    })
}

/// Where an in-flow asset's export sits inside the box the layout gives it,
/// and how large it is: the render bounds against the bounding box. `None`
/// when they coincide, which is every plain icon, or when the snapshot does
/// not carry them.
pub(super) fn export_offset(node: &RawNode) -> Option<Box4> {
    let view = node.typed_view();
    let render = box4(view.value("absoluteRenderBounds"))?;
    let bounds = box4(view.value("absoluteBoundingBox"))?;
    let close = |left: f64, right: f64| (left - right).abs() < 0.5;
    if close(render.x, bounds.x)
        && close(render.y, bounds.y)
        && close(render.w, bounds.w)
        && close(render.h, bounds.h)
    {
        return None;
    }
    Some(Box4 {
        x: render.x - bounds.x,
        y: render.y - bounds.y,
        w: render.w,
        h: render.h,
    })
}

/// The box the layout gives an asset that is not positioned: its bounding
/// box, which is its own box unless it is rotated, when it is the box the
/// rotation sweeps - the box Figma's own layout gives it.
pub(crate) fn layout_box(node: &RawNode) -> Option<Box4> {
    box4(node.typed_view().value("absoluteBoundingBox"))
}

// Root placement, width policy and image representation are independent
// inputs to this shared layout property writer.
#[allow(clippy::too_many_arguments)]
pub(super) fn push_layout_props(
    snapshot: &Snapshot,
    node: &RawNode,
    component: &str,
    props: &mut Vec<Prop>,
    root_layout: RootLayout,
    is_render_root: bool,
    policy: Option<&WidthPolicy>,
    original_source: bool,
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
    let absolute = view.string("layoutPositioning") == Some("ABSOLUTE")
        || placed_by_a_free_layout(snapshot, node, parent, is_page_root);
    let embedded_root = is_render_root && root_layout == RootLayout::Embedded;
    let containing_block = holds_positioned_children(snapshot, node);
    let mut width = None;
    let mut height = None;
    let mut policy_sizing = None;

    if embedded_root {
        // The selected frame is being inserted into an existing page layout.
        // Preserve its visual/layout semantics, but do not constrain the host
        // with Figma canvas geometry or root positioning.
    } else if absolute {
        let is_asset = !original_source && super::style::asset_kind(snapshot, node).is_some();
        // An exported asset is placed by what the export frames. The plugin
        // places it by the node's own box and rotates it again, and the
        // report section's rotated illustration landed 120px low, squeezed
        // into 686x735 where its export is 759x585.
        let export = is_asset.then(|| export_box(snapshot, node)).flatten();
        let anchor = push_absolute(snapshot, node, parent, props, export, policy);
        // Figma paints children in order, so a pinned picture drawn before
        // its siblings sits behind them. CSS paints a positioned element
        // after every in-flow sibling whatever the order, so the landing
        // page's hero picture came out over its headline and the join-us
        // badges over their buttons. Sent behind with `zIndex="-1"` - inside
        // the stacking context the parent opens for it, below - it sits
        // where Figma has it: above the parent's own background, under the
        // content.
        //
        // Only under a parent that opens that context, including page roots.
        if let Some(parent) = parent
            && holds_positioned_children(snapshot, parent)
            && sits_behind_in_flow_siblings(snapshot, parent, &node.id)
        {
            string_prop(props, "zIndex", "-1");
        }
        if let Some(export) = export {
            width = Some(px(export.w));
            height = Some(px(export.h));
        }
        // The plugin's `_getLayoutProps` for a positioned node, as one rule
        // rather than a branch per kind of node. Its width is its own only
        // while the parent is wider and it is an asset or an empty frame;
        // spilling past the parent it is `100%`, and a frame with children in
        // it has no width said at all, its children being what sizes it. Its
        // height is `100%` for a shape, its own for an empty frame, and unsaid
        // once it has children. So the about hero picture, 418px in a 320px
        // column, is `boxSize="100%"`, and a hidden 1920px frame in a 992px
        // one is `w="100%" h="667px"`.
        //
        // Preserve the existing placement policy here. Only child-folded mask
        // HUG axes are repaired below; other unproven sizes remain diagnostics.
        let own_width = view.number("width");
        let parent_width = parent.and_then(|parent| parent.typed_view().number("width"));
        if export.is_some() {
            // Sized above, by the export.
        } else if component == "Text" {
            width = own_width.map(px);
            height = Some("100%".to_owned());
        } else if view.node_type() == "INSTANCE" && is_asset {
            // The plugin puts a positioned instance in a wrapper `Box` that
            // carries the position, and draws the instance from its main
            // component, which sizes itself by its own layout. An instance
            // that folds to an asset is one element here, so it takes that
            // size: a 651px logo pinned in a 360px banner is
            // `w="651px" h="46px"`, not `w="100%"`. An instance kept as a
            // component reference is sized as the wrapper is, below.
            if fixed_w {
                width = own_width.map(px);
            }
            if fixed_h {
                height = view.number("height").map(px);
            }
        } else {
            let parent_wider = matches!(
                (own_width, parent_width),
                (Some(width), Some(parent_width)) if parent_width > width
            );
            let has_children = view.child_ids().next().is_some();
            let holds_children = matches!(
                view.node_type(),
                "FRAME"
                    | "GROUP"
                    | "INSTANCE"
                    | "COMPONENT"
                    | "COMPONENT_SET"
                    | "BOOLEAN_OPERATION"
            );
            let empty_frame = holds_children && !has_children;
            width = if parent_wider {
                (is_asset || empty_frame)
                    .then(|| own_width.map(px))
                    .flatten()
            } else {
                Some("100%".to_owned())
            };
            let wider_than_parent = matches!(
                (own_width, parent_width),
                (Some(width), Some(parent_width)) if width >= parent_width
            );
            // A shape with nothing in it is as big as itself. `100%` is
            // only the same thing when it covers its parent - the popup's
            // dim overlay - and a 220px circle pinned in an 1,102px group
            // said `h="100%"` and no width at all, which is no circle.
            //
            // Only inside a group. The plugin's rule says `100%` and no
            // width for a small shape pinned in a frame as well, and the
            // pinned corpus holds four such shapes; that is as wrong there,
            // but no rendered screen in the corpus shows it, so it keeps
            // byte parity until one does. A group is a different case in
            // any event: it draws nothing and lays nothing out, so a shape
            // in it can only ever be its own size.
            let own_height = view.number("height");
            let parent_height = parent.and_then(|parent| parent.typed_view().number("height"));
            let parent_taller = matches!(
                (own_height, parent_height),
                (Some(height), Some(parent_height)) if parent_height > height
            );
            let in_a_group =
                parent.is_some_and(|parent| parent.typed_view().node_type() == "GROUP");
            let leaf_shape = !has_children && !holds_children && !is_asset && in_a_group;
            if leaf_shape && parent_wider {
                width = own_width.map(px);
            }
            height = if has_children {
                (is_asset && !wider_than_parent)
                    .then(|| own_height.map(px))
                    .flatten()
            } else if empty_frame || (leaf_shape && parent_taller) {
                own_height.map(px)
            } else {
                Some("100%".to_owned())
            };
        }
        // An absolutely positioned node is out of flow, so nothing constrains
        // it from the outside and the branches above may leave it sizeless,
        // expecting its children to define the box. That is wrong whenever
        // Figma pinned the size and nothing else accounts for it — a folded
        // asset has no children left to measure at all. Where the gap around
        // the children became padding, though, that padding and the content
        // already add back up to the frame, and restating the size only says
        // it twice.
        //
        // Ordinary frames may derive size from their content or padding.
        // Child-folded HUG masks receive a bounded size repair below.
        if fixed_w
            && fixed_h
            && width.is_none()
            && height.is_none()
            && derived_padding(snapshot, node).is_none()
        {
            width = view.number("width").map(px);
            height = view.number("height").map(px);
        }
        // The same fact on the height alone, for a frame that fills its
        // parent's width and so was given one above. Its children size it in
        // CSS where Figma pinned it: the notice header is 60 tall around a
        // 24px row of logo and menu, and centring them in 24 rather than 60
        // put them 18px high of where Figma draws them.
        //
        // Child-folded HUG masks have a repair below. A FIXED container height
        // is independent of the current child's HUG height and derived insets.
        if fixed_h && height.is_none() && !is_asset && view.child_ids().next().is_some() {
            // Derived insets describe the captured child geometry, not a height
            // constraint. HUG content can change without changing this FIXED box.
            height = view.number("height").map(px);
        }
        policy_sizing = anchor.zip(parent).map(|(anchor, parent)| {
            let parent_view = parent.typed_view();
            (
                anchor,
                super::width::placed_box(node, parent, export),
                parent_view.number("width").unwrap_or(0.0),
                parent_view.number("height").unwrap_or(0.0),
                is_asset,
            )
        });
    } else if is_page_root {
        // Figma page roots define the component canvas; their editor dimensions
        // are not emitted as runtime constraints: a root's width is the
        // viewport's, and a root that lays its children out is as tall as
        // they are, at any width.
        //
        // A root that lays nothing out has nothing in flow to give it a
        // height — its children are placed absolutely, or its one child is
        // centred in it. The plugin leaves it sizeless too, and the popup
        // overlay, a 390×800 frame dimmed behind one centred card, comes out
        // a box of no height whose dim is never drawn; the answer it wrote
        // gave the height back as padding, `py="211.5px"` around a 377px
        // card. The drawn height is the design, and it is kept, one value per
        // width. The width is still the viewport's. This departs from the
        // plugin on purpose.
        if lays_nothing_out(node)
            && view.child_ids().next().is_some()
            && derived_padding(snapshot, node).is_none()
        {
            height = view.number("height").map(px);
        }
    } else if fixed_w || fixed_h {
        if fixed_w {
            width = view.number("width").map(px);
        }
        if fixed_h {
            height = view.number("height").map(px);
        }
        // A rotated asset in flow takes the box its rotation sweeps, which
        // is the box Figma's layout gives it and the box its export fills;
        // its own width and height are the picture's before the turn.
        if view
            .number("rotation")
            .is_some_and(|rotation| rotation.abs() > 0.01)
            && super::style::asset_kind(snapshot, node).is_some()
            && !original_source
            && let Some(bounds) = layout_box(node)
        {
            if fixed_w {
                width = Some(px(bounds.w));
            }
            if fixed_h {
                height = Some(px(bounds.h));
            }
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

    // A canvas with positioned children is their coordinate system, not just
    // a responsive page measurement. Preserve each FIXED axis even on a page
    // or section root. Embedded mode explicitly delegates dimensions to host.
    if containing_block && !embedded_root {
        if fixed_w {
            width = view.number("width").map(px);
        }
        if fixed_h {
            height = view.number("height").map(px);
        }
    }

    // A FIXED column is also the height basis for in-flow FILL children.
    // This relationship does not depend on positioned descendants (R6).
    if !embedded_root && vertical_fill_container(snapshot, node) {
        height = view.number("height").map(px);
    }
    let vertical_fill = !absolute
        && !embedded_root
        && fill_h
        && parent.is_some_and(|p| p.typed_view().string("layoutMode") == Some("VERTICAL"));
    if vertical_fill {
        // Main-axis FILL means remaining space, not 100% of the whole parent.
        height = None;
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

    // Only folding children into a CSS mask destroys HUG's intrinsic basis.
    // FILL remains fluid; a vector's measured ratio supplies its lost HUG axis.
    let mut restored_ratio = None;
    if !embedded_root && let Some((w, h)) = folded_mask_dimensions(snapshot, node) {
        let horizontal = view.string("layoutSizingHorizontal");
        let vertical = view.string("layoutSizingVertical");
        if horizontal == Some("FILL") && vertical == Some("HUG") {
            width = Some("100%".into());
            height = None;
            restored_ratio = Some(format!("{w} / {h}"));
        } else if horizontal == Some("HUG") && vertical == Some("FILL") {
            width = None;
            height = Some("100%".into());
            restored_ratio = Some(format!("{w} / {h}"));
        } else {
            if horizontal == Some("HUG") {
                width = Some(px(w));
            }
            if vertical == Some("HUG") {
                height = Some(px(h));
            }
        }
    }

    // Horizontal FILL is independent of the other axis. Parent alignment
    // can disable implicit stretch; preserve the fluid allocation explicitly.
    if !absolute
        && !embedded_root
        && fill_w
        && width.is_none()
        && parent.is_some_and(|p| p.typed_view().string("layoutMode") == Some("VERTICAL"))
        && parent.is_some_and(|p| {
            matches!(
                p.typed_view().string("counterAxisAlignItems"),
                Some("CENTER" | "MAX" | "BASELINE")
            )
        })
    {
        width = Some("100%".into());
    }

    // Cross-axis FILL must stretch even when the row centers other children.
    // Percentage height cannot resolve against a HUG row's auto height.
    if !absolute
        && !embedded_root
        && fill_h
        && parent.is_some_and(|p| {
            p.typed_view().string("layoutMode") == Some("HORIZONTAL")
                && p.typed_view().string("layoutSizingVertical") == Some("HUG")
        })
    {
        height = None;
        string_prop(props, "alignSelf", "stretch");
    }

    // Under a width policy the horizontal anchor decides the width, after
    // every other rule: a stretch is its two edges, a scale its share of the
    // parent, and any other anchor holds a childless shape at its own drawn
    // size, which the plugin's `h="100%"` and no width does not - except on an
    // axis where it spans its whole parent, which keeps following it as the
    // default export already writes it.
    if let Some((anchor, placed, parent_width, parent_height, is_asset)) = policy_sizing {
        let shape = component != "Text" && !is_asset && view.child_ids().next().is_none();
        match anchor {
            Anchor::Stretch => width = None,
            Anchor::Scale => width = Some(super::width::percent(placed.w, parent_width)),
            _ if shape && placed.w + 0.5 < parent_width => width = view.number("width").map(px),
            _ => {}
        }
        if shape && placed.h + 0.5 < parent_height {
            height = view.number("height").map(px);
        }
    }
    // A root under a width policy takes its container's width; the drawn
    // width stays the coordinate basis its positioned children were placed
    // in, and the widthBehavior report says what that leaves unproven.
    if policy.is_some()
        && is_render_root
        && !embedded_root
        && width.as_deref().is_some_and(|width| width.ends_with("px"))
    {
        width = Some("100%".to_owned());
    }
    // Whether the height was said outright, which decides below whether the
    // node still needs to be told to take the space its parent leaves.
    if original_source && absolute {
        if fixed_w {
            width = view.number("width").map(px);
        }
        if fixed_h {
            height = view.number("height").map(px);
        }
    }
    let wrote_height = height.is_some();
    let wrote_width = width.is_some();
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

    // A positioned node has its size said outright — the plugin's absolute
    // branch of `_getLayoutProps` never writes `aspectRatio` — so the ratio
    // is only for a node in flow, where it stands in for a side that is not
    // written. The about hero picture is `boxSize="100%"` with no ratio.
    if let Some(ratio) = restored_ratio {
        string_prop(props, "aspectRatio", ratio);
    } else if !absolute
        && let Some(aspect) = view.value("targetAspectRatio").and_then(Value::as_object)
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
        // A component set's grid is how Figma lays its variants out on the
        // canvas, not how the component is built. A variant is drawn on its
        // own wherever it is used, so carrying the cell it sat in would place
        // every button at the coordinates of its row in the sheet.
        && parent.typed_view().node_type() != "COMPONENT_SET"
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
        // How many tracks the child covers. The plugin writes `span 1` for
        // every child, and a picture drawn across two columns came out in one,
        // its neighbour pushed a row down and every `1fr` row stretched to
        // the tallest picture: a 1086px grid rendered 2191px tall. The spans
        // are Figma's own, absent from the snapshot when they are 1.
        let column_span = view.number("gridColumnSpan").unwrap_or(1.0).max(1.0);
        let row_span = view.number("gridRowSpan").unwrap_or(1.0).max(1.0);
        let column_count = parent.typed_view().number("gridColumnCount").unwrap_or(0.0);
        let current = column + row * column_count;
        let natural = parent
            .typed_view()
            .child_ids()
            .position(|child| child == node.id)
            .map(|index| index as f64);
        // A child at its natural cell needs no placement - unless it spans,
        // which flow alone would not give it.
        if column >= 0.0
            && row >= 0.0
            && (natural != Some(current) || column_span > 1.0 || row_span > 1.0)
        {
            string_prop(
                props,
                "gridColumn",
                format!(
                    "{} / span {}",
                    format_number(column + 1.0),
                    format_number(column_span)
                ),
            );
            string_prop(
                props,
                "gridRow",
                format!(
                    "{} / span {}",
                    format_number(row + 1.0),
                    format_number(row_span)
                ),
            );
        }
    }
    if fill_w
        && parent
            .is_some_and(|parent| parent.typed_view().string("layoutMode") == Some("HORIZONTAL"))
    {
        string_prop(props, "flex", "1");
        // `flex: 1` shares out the room left over, and Figma gives a filling
        // child exactly that - here 123px - and lets anything wider inside it
        // spill out. CSS will not shrink a flex item under its content
        // (`min-width: auto`), so a 132px row inside a 123px share widened
        // the share to 132 and took the 9px from the hugging sibling, whose
        // `Devup-ui` then broke across two lines. `minW="0"` lets the share
        // be the share, and the content spill as Figma draws it. Written
        // only where the snapshot shows content wider than the box. A row
        // can overflow collectively even when each child fits individually.
        // Wrapping rows can distribute that content over multiple lines.
        let child_overflows = view.number("width").is_some_and(|own| {
            view.child_ids()
                .filter_map(|id| snapshot.nodes.get(id))
                .any(|child| {
                    let child = child.typed_view();
                    child.bool("visible") != Some(false)
                        && child.string("layoutPositioning") != Some("ABSOLUTE")
                        && child.number("width").is_some_and(|width| width > own + 0.5)
                })
        });
        let row_overflows = view.string("layoutMode") == Some("HORIZONTAL")
            && view.string("layoutWrap") != Some("WRAP")
            && line_overflows(snapshot, node, "width", "paddingLeft", "paddingRight");
        if view.number("minWidth").is_none() && (child_overflows || row_overflows) {
            string_prop(props, "minW", "0");
        }
    }
    // Main-axis FILL consumes remaining space, whether its own children are
    // positioned or in flow. This keeps the completion screen's body ahead of
    // the following bottom action without copying any canvas coordinates.
    if vertical_fill {
        string_prop(props, "flex", "1");
        if vertical_fill_needs_minimum_reset(snapshot, node) {
            string_prop(props, "minH", "0");
        }
    }
    // A child Figma never shrinks, in a line that does not fit. Figma keeps a
    // fixed size and lets the row spill past its parent, which clips it; CSS
    // shrinks flex children to fit instead. The devup-ui landing page's
    // comparison row is seven 240px cards in a 912px frame - 1,800px of
    // content - and every one of them was squeezed to about 120px, their
    // labels wrapped to two lines, and the row came out 58px taller than the
    // design, carrying everything below it down with it.
    //
    // Only where the line actually overflows. Children that fit are not
    // shrunk by CSS either, and saying so for every fixed child in the file
    // would be noise.
    if !absolute
        && let Some(parent) = parent
        && let Some(axis) = parent.typed_view().string("layoutMode")
        && matches!(axis, "VERTICAL" | "HORIZONTAL")
    {
        let (sizing, size, near, far) = if axis == "HORIZONTAL" {
            (
                "layoutSizingHorizontal",
                "width",
                "paddingLeft",
                "paddingRight",
            )
        } else {
            (
                "layoutSizingVertical",
                "height",
                "paddingTop",
                "paddingBottom",
            )
        };
        if view.string(sizing) == Some("FIXED") && line_overflows(snapshot, parent, size, near, far)
        {
            string_prop(props, "flexShrink", "0");
        }
    }
    // A child that hugs across its parent's axis, in a parent that packs its
    // children to the start, drawn narrower than the room it has, and drawn
    // differently for being stretched into it.
    //
    // Figma's default counter-axis alignment is MIN, which it writes by
    // leaving the field out - and leaving it out of the code too means CSS
    // applies its own default, `align-items: stretch`, which is the
    // opposite. The devup-ui landing page's `Get started` button, 247px wide
    // in the 1360px column that holds it, was drawn 1360px wide.
    //
    // Most hugging children do not care. A line of left-aligned text in a
    // box that paints nothing is the same picture at any width, and writing
    // an alignment for every one of them - the notice desktop alone holds
    // twenty - says nothing while burying the few that matter. So it is
    // written where the wider box would show: where the node paints across
    // it, or places its own content by it.
    if !absolute
        && let Some(parent) = parent
        && let Some(axis) = parent.typed_view().string("layoutMode")
        && matches!(axis, "VERTICAL" | "HORIZONTAL")
        && matches!(
            parent.typed_view().string("counterAxisAlignItems"),
            None | Some("MIN")
        )
    {
        let across_is_horizontal = axis == "VERTICAL";
        let (sizing, size, near, far, wrote) = if across_is_horizontal {
            (
                "layoutSizingHorizontal",
                "width",
                "paddingLeft",
                "paddingRight",
                wrote_width,
            )
        } else {
            (
                "layoutSizingVertical",
                "height",
                "paddingTop",
                "paddingBottom",
                wrote_height,
            )
        };
        if view.string(sizing) == Some("HUG")
            && !wrote
            && let (Some(own), Some(room)) =
                (view.number(size), inner_extent(parent, size, near, far))
            && own + 0.5 < room
            && a_wider_box_would_show(&view, across_is_horizontal)
        {
            string_prop(props, "alignSelf", "flex-start");
        }
    }

    push_auto_layout(snapshot, node, component, props);
    push_padding(snapshot, node, props);
    match view.string("overflowDirection") {
        Some("VERTICAL_SCROLLING") => {
            string_prop(props, "overflowY", "auto");
            if view.bool("clipsContent") == Some(true) {
                string_prop(props, "overflowX", "hidden");
            }
        }
        Some("HORIZONTAL_SCROLLING") => {
            string_prop(props, "overflowX", "auto");
            if view.bool("clipsContent") == Some(true) {
                string_prop(props, "overflowY", "hidden");
            }
        }
        Some("HORIZONTAL_AND_VERTICAL_SCROLLING") => string_prop(props, "overflow", "auto"),
        _ if view.bool("clipsContent") == Some(true) => string_prop(props, "overflow", "hidden"),
        _ => {}
    }
    // An absolutely positioned child needs a positioned ancestor to resolve
    // against — but a node folded into a single asset has no children left in
    // the output, so there is nothing to anchor and the containing block would
    // exist for no one.
    //
    // A node that is itself positioned is already that ancestor, and saying
    // `relative` over its `absolute` would put it back in flow: the join-us
    // group of circles, pinned at -277,-187, took 1,102px of the page.
    if containing_block && (!absolute || embedded_root) {
        string_prop(props, "pos", "relative");
    }
    // Position (relative or absolute) establishes a containing block, but
    // needs an explicit z-index to contain negative-z children above its own
    // background. Keep an existing z-index that places this entire subtree.
    if containing_block
        && !props.iter().any(|(name, _)| name == "zIndex")
        && view
            .child_ids()
            .any(|child| sits_behind_in_flow_siblings(snapshot, node, child))
    {
        string_prop(props, "zIndex", "0");
    }
    // An export is drawn with its rotation in it, so an asset whose bounds
    // the snapshot carries is not rotated again; without the bounds it is
    // placed as the plugin places it, rotation and all.
    let exported = !original_source
        && super::style::asset_kind(snapshot, node).is_some()
        && view.value("absoluteRenderBounds").is_some();
    if let Some(rotation) = view.number("rotation")
        && rotation.abs() > 0.01
        && !exported
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
    if let Some(grid) = reading_grid(snapshot, node) { reading_grid_props(&grid, props); }
    reading_card_props(snapshot, node, props);
}

pub(crate) fn vertical_fill_container(snapshot: &Snapshot, node: &RawNode) -> bool {
    let view = node.typed_view();
    view.string("layoutMode") == Some("VERTICAL")
        && view.string("layoutSizingVertical") == Some("FIXED")
        && view
            .child_ids()
            .filter_map(|id| snapshot.nodes.get(id))
            .any(|child| {
                let child = child.typed_view();
                child.bool("visible") != Some(false)
                    && child.string("layoutPositioning") != Some("ABSOLUTE")
                    && child.string("layoutSizingVertical") == Some("FILL")
            })
}

/// CSS's automatic flex minimum must not enlarge a captured FILL allocation.
/// Preserve explicit source minimums; only reset the implicit content minimum
/// when collected in-flow content demonstrably overflows the allocated height.
pub(crate) fn vertical_fill_needs_minimum_reset(snapshot: &Snapshot, node: &RawNode) -> bool {
    let view = node.typed_view();
    if view.number("minHeight").is_some() {
        return false;
    }
    (view.string("layoutMode") == Some("VERTICAL")
        && line_overflows(snapshot, node, "height", "paddingTop", "paddingBottom"))
        || inner_extent(node, "height", "paddingTop", "paddingBottom").is_some_and(|room| {
            view.child_ids()
                .filter_map(|id| snapshot.nodes.get(id))
                .any(|child| {
                    let child = child.typed_view();
                    child.bool("visible") != Some(false)
                        && child.string("layoutPositioning") != Some("ABSOLUTE")
                        && child
                            .number("height")
                            .is_some_and(|height| height > room + 0.5)
                })
        })
}

/// Only children that can survive projection need a containing block.
pub(super) fn holds_positioned_children(snapshot: &Snapshot, node: &RawNode) -> bool {
    super::style::asset_kind(snapshot, node).is_none()
        && node.typed_view().child_ids().any(|id| {
            snapshot.nodes.get(id).is_some_and(|child| {
                let view = child.typed_view();
                view.bool("visible") != Some(false)
                    && (view.string("layoutPositioning") == Some("ABSOLUTE")
                        || placed_by_a_free_layout(snapshot, child, Some(node), false))
            })
        })
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

pub(super) struct GridTemplate {
    pub css: Option<String>,
    pub unverified: Vec<&'static str>,
}