use devup_mcp_figma::{RawNode, Snapshot, TypedNode};
use serde_json::Value;

use super::component::{Prop, PropValue, RootLayout};

/// A box in a parent's coordinates: left, top, width, height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Box4 {
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
pub(super) fn export_box(snapshot: &Snapshot, node: &RawNode) -> Option<Box4> {
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
pub(super) fn layout_box(node: &RawNode) -> Option<Box4> {
    box4(node.typed_view().value("absoluteBoundingBox"))
}

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
    let absolute = view.string("layoutPositioning") == Some("ABSOLUTE")
        || placed_by_a_free_layout(snapshot, node, parent, is_page_root);
    let embedded_root = is_render_root && root_layout == RootLayout::Embedded;
    let mut width = None;
    let mut height = None;

    if embedded_root {
        // The selected frame is being inserted into an existing page layout.
        // Preserve its visual/layout semantics, but do not constrain the host
        // with Figma canvas geometry or root positioning.
    } else if absolute {
        let is_asset = super::style::asset_kind(snapshot, node).is_some();
        // An exported asset is placed by what the export frames. The plugin
        // places it by the node's own box and rotates it again, and the
        // report section's rotated illustration landed 120px low, squeezed
        // into 686x735 where its export is 759x585.
        let export = is_asset.then(|| export_box(snapshot, node)).flatten();
        push_absolute(snapshot, node, parent, props, export);
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
        // One departure, on purpose. An asset folds its children away and is
        // drawn at a size, and the plugin still says no height for it: the
        // 465px puzzle icon comes out `w="465px"` alone, a mask with nothing
        // to mask. Here it keeps its height — unless it is wider than its
        // parent, where the pinned corpus wants `w="100%"` and no height, and
        // `provenance` expects the same. Text keeps its own width and `100%`:
        // the plugin says nothing for a positioned text and the corpus wants
        // the size written.
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
        // Both sides or neither, on purpose. An absolute asset wider than its
        // parent gets w="100%" and no height above, and the pinned corpus
        // wants exactly that — two goldens carry a full-width rotated
        // background mask with no h, and restoring the height there breaks
        // byte parity. The box has no height and draws by its mask alone;
        // that is the reference's rule, and it is matched rather than fixed.
        //
        // This is a departure from the plugin, which says no size for a
        // positioned frame with children and lets them size it. A 12px box
        // holding a 2px dot at its centre then collapses to the dot, and the
        // dot lands 5px off; the pinned size is a layout fact, and it is kept.
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
        // An asset is left out, as it is above: it has no children left to
        // measure, and the two goldens carrying a full-width rotated mask
        // want their height unsaid. So is a frame whose spare room became
        // padding, which already adds back up to the pinned height.
        if fixed_h
            && height.is_none()
            && !is_asset
            && view.child_ids().next().is_some()
            && derived_padding(snapshot, node).is_none()
        {
            height = view.number("height").map(px);
        }
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

    // Whether the height was said outright, which decides below whether the
    // node still needs to be told to take the space its parent leaves.
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
    if !absolute
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
    }
    // The same along the other axis, for the one node CSS cannot size on its
    // own. A node set to fill its parent's main axis is stretched by Figma to
    // the space left over; said nothing about, CSS lets it hug its content
    // instead. That usually agrees - a column of in-flow children adds up to
    // the height Figma gave it - but a positioned child adds nothing to the
    // height of what holds it, so hugging can never reach it. The about
    // page's hero column is 440 tall in a 520 tall section and came out 155,
    // the height of its text alone; the section then centred that, pushing it
    // 143px down and dropping the picture hung off it over the heading it is
    // meant to sit above.
    let holds_a_positioned_child = view.child_ids().any(|child_id| {
        snapshot
            .nodes
            .get(child_id)
            .is_some_and(|child| child.typed_view().string("layoutPositioning") == Some("ABSOLUTE"))
    });
    if fill_h
        && !wrote_height
        && holds_a_positioned_child
        && parent.is_some_and(|parent| parent.typed_view().string("layoutMode") == Some("VERTICAL"))
    {
        string_prop(props, "flex", "1");
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
    if view.bool("clipsContent") == Some(true) {
        string_prop(props, "overflow", "hidden");
    }
    // An absolutely positioned child needs a positioned ancestor to resolve
    // against — but a node folded into a single asset has no children left in
    // the output, so there is nothing to anchor and the containing block would
    // exist for no one.
    //
    // A node that is itself positioned is already that ancestor, and saying
    // `relative` over its `absolute` would put it back in flow: the join-us
    // group of circles, pinned at -277,-187, took 1,102px of the page.
    if !embedded_root
        && !is_page_root
        && !absolute
        && super::style::asset_kind(snapshot, node).is_none()
        && view.child_ids().any(|child| {
            snapshot.nodes.get(child).is_some_and(|child| {
                child.typed_view().string("layoutPositioning") == Some("ABSOLUTE")
                    || placed_by_a_free_layout(snapshot, child, Some(node), false)
            })
        })
    {
        string_prop(props, "pos", "relative");
    }
    // An export is drawn with its rotation in it, so an asset whose bounds
    // the snapshot carries is not rotated again; without the bounds it is
    // placed as the plugin places it, rotation and all.
    let exported = super::style::asset_kind(snapshot, node).is_some()
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

/// A grid's tracks as CSS, from Figma's track sizes: a `FLEX` track is its
/// share in `fr`, a `FIXED` one its pixels, a `HUG` one `fit-content(100%)`,
/// as Figma's own documentation maps them. Tracks all alike fold to
/// `repeat(n, …)`. The plugin writes `repeat(n, 1fr)` for every grid, which
/// is right only while every track is one flexible share; without the sizes
/// in the snapshot that is what this falls back to.
fn grid_template(sizes: Option<&Value>, count: f64) -> String {
    let tracks = sizes
        .and_then(Value::as_array)
        .map(|tracks| {
            tracks
                .iter()
                .map(|track| {
                    let value = track.get("value").and_then(Value::as_f64);
                    match track.get("type").and_then(Value::as_str) {
                        Some("FIXED") => px(value.unwrap_or(0.0)),
                        Some("HUG") => "fit-content(100%)".to_owned(),
                        _ => format!("{}fr", format_number(value.unwrap_or(1.0))),
                    }
                })
                .collect::<Vec<_>>()
        })
        .filter(|tracks| !tracks.is_empty())
        .unwrap_or_else(|| vec!["1fr".to_owned(); count.max(0.0) as usize]);
    // `repeat(1, 1fr)` for one track too: that is the plugin's spelling, and
    // the corpus holds it.
    if !tracks.is_empty() && tracks.iter().all(|track| track == &tracks[0]) {
        format!("repeat({}, {})", tracks.len(), tracks[0])
    } else {
        tracks.join(" ")
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
            grid_template(
                view.value("gridColumnSizes"),
                view.number("gridColumnCount").unwrap_or(0.0),
            ),
        );
        string_prop(
            props,
            "gridTemplateRows",
            grid_template(
                view.value("gridRowSizes"),
                view.number("gridRowCount").unwrap_or(0.0),
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
    // Figma's two newer distributions came to the plugin after the pinned
    // corpus (9214391); they are the CSS keywords of the same name.
    let justify = match view.string("primaryAxisAlignItems") {
        Some("MIN") => None,
        Some("MAX") => Some("flex-end"),
        Some("CENTER") => Some("center"),
        Some("SPACE_BETWEEN") => Some("space-between"),
        Some("SPACE_AROUND") => Some("space-around"),
        Some("SPACE_EVENLY") => Some("space-evenly"),
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
/// The padding this node will actually be given from its children's placement.
///
/// A folded asset is excluded: its children are baked into the exported image
/// and never laid out, so measuring a gap around them would describe a box
/// nothing lives in.
/// How much room a frame leaves its children across one axis: its own size
/// less the padding on that axis. `None` when the size is not recorded.
fn inner_extent(node: &RawNode, size: &str, near: &str, far: &str) -> Option<f64> {
    let view = node.typed_view();
    let inferred = view.value("inferredAutoLayout").and_then(Value::as_object);
    let padding = |name: &str| {
        inferred
            .and_then(|layout| layout.get(name))
            .and_then(Value::as_f64)
            .or_else(|| view.number(name))
            .unwrap_or(0.0)
    };
    Some(view.number(size)? - padding(near) - padding(far))
}

/// Whether a node drawn into a wider box than it asked for would look any
/// different: either it paints across the box - a fill, a stroke, a shadow -
/// or it places its own content by the box's far edge or centre along that
/// axis. A left-aligned line of text in a box that paints nothing does not.
fn a_wider_box_would_show(view: &TypedNode<'_>, across_is_horizontal: bool) -> bool {
    let visible = |value: Option<&Value>| {
        value.and_then(Value::as_array).is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry.get("visible").and_then(Value::as_bool) != Some(false)
                    && entry.get("type").and_then(Value::as_str) != Some("NONE")
            })
        })
    };
    // A text node's fills are its ink, not its box, and a shadow follows the
    // glyphs: none of them widen with the box. A centred line would move,
    // and the pinned corpus holds three such texts - but no screen in the
    // corpus renders one, so there is nothing to show that writing it helps,
    // and it costs byte parity with the plugin on all three. Left alone
    // until a rendered screen asks for it.
    if view.node_type() == "TEXT" {
        return false;
    }
    if visible(view.value("fills"))
        || visible(view.value("strokes"))
        || visible(view.value("effects"))
    {
        return true;
    }
    // Along its own main axis a node is placed by `primaryAxisAlignItems`,
    // across it by `counterAxisAlignItems`; which of the two answers for the
    // axis being stretched depends on which way the node itself runs.
    let along_its_main_axis = match view.string("layoutMode") {
        Some("HORIZONTAL") => across_is_horizontal,
        Some("VERTICAL") => !across_is_horizontal,
        _ => return false,
    };
    if along_its_main_axis {
        matches!(
            view.string("primaryAxisAlignItems"),
            Some("CENTER" | "MAX" | "SPACE_BETWEEN" | "SPACE_AROUND" | "SPACE_EVENLY")
        )
    } else {
        matches!(view.string("counterAxisAlignItems"), Some("CENTER" | "MAX"))
    }
}

/// Whether a frame's children, laid end to end along its own axis with the
/// gaps between them, come to more than the room it leaves. Figma lets them
/// spill and clips; CSS shrinks them to fit, so the two only agree while
/// they fit. Children out of flow or not drawn take no room.
fn line_overflows(snapshot: &Snapshot, node: &RawNode, size: &str, near: &str, far: &str) -> bool {
    let view = node.typed_view();
    let Some(room) = inner_extent(node, size, near, far) else {
        return false;
    };
    let children = view
        .child_ids()
        .filter_map(|id| snapshot.nodes.get(id))
        .filter(|child| {
            let child = child.typed_view();
            child.bool("visible") != Some(false)
                && child.string("layoutPositioning") != Some("ABSOLUTE")
        })
        .collect::<Vec<_>>();
    if children.len() < 2 {
        return false;
    }
    let gap = view
        .value("inferredAutoLayout")
        .and_then(Value::as_object)
        .and_then(|layout| layout.get("itemSpacing"))
        .and_then(Value::as_f64)
        .or_else(|| view.number("itemSpacing"))
        .unwrap_or(0.0);
    let mut extent = gap * (children.len() - 1) as f64;
    for child in children {
        let Some(own) = child.typed_view().number(size) else {
            return false;
        };
        extent += own;
    }
    extent > room + 0.5
}

pub(crate) fn derived_padding(snapshot: &Snapshot, node: &RawNode) -> Option<[f64; 4]> {
    let view = node.typed_view();
    // Figma reports a frame it cannot infer a layout for as an explicit null,
    // so presence alone does not mean there is a layout to read.
    if view
        .value("inferredAutoLayout")
        .and_then(Value::as_object)
        .is_some()
        || view.string("layoutMode") != Some("NONE")
    {
        return None;
    }
    if super::style::asset_kind(snapshot, node).is_some() {
        return None;
    }
    // Padding places one child. Two or more at their own positions cannot be
    // put back by an inset around all of them — in flow they would stack —
    // so they are placed one by one instead, see `placed_by_a_free_layout`.
    only_visible_child(snapshot, node)?;
    // A child the designer centred is centred, not padded: see
    // `centres_its_only_child`.
    if centres_its_only_child(snapshot, node) {
        return None;
    }
    children_inset(snapshot, node)
}

/// The one visible child of a frame, when there is exactly one.
fn only_visible_child<'a>(snapshot: &'a Snapshot, node: &RawNode) -> Option<&'a RawNode> {
    let view = node.typed_view();
    let mut visible = view
        .child_ids()
        .filter_map(|id| snapshot.nodes.get(id))
        .filter(|child| child.typed_view().bool("visible") != Some(false));
    match (visible.next(), visible.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

/// Whether a frame that lays nothing out holds one child the designer centred
/// on both axes — constraints `CENTER` / `CENTER`.
///
/// Such a child is centred by its constraint, not by the inset it happened to
/// have at the width it was drawn: the popup card sits 36.5px in at 390px and
/// 742px in at 1920px, and a measured `pl="36.5px"` holds at the one width
/// and drifts at every other. The frame is written as a `Center` with the
/// child in flow, which centres it at any width and needs no positioned
/// ancestor; a page root keeps its drawn height so there is something to
/// centre in, see `push_layout_props`.
///
/// This departs from the plugin, on purpose. Its `canBeAbsolute` writes the
/// child `pos="absolute" left="50%" top="50%" transform="translate(-50%,
/// -50%)"`, which resolves against whatever positioned ancestor the page is
/// given, and leaves the frame — a page root, so sizeless — a box of no
/// height whose dim is never drawn. The answer it wrote for the popup, from a
/// layout Figma has since stopped inferring, pads the card in by the inset at
/// each width and at desktop by none, which leaves the card at the left of a
/// 1920px screen.
///
/// A child centred on one axis only, or pinned to an edge, keeps the measured
/// inset as padding, as before.
pub(crate) fn centres_its_only_child(snapshot: &Snapshot, node: &RawNode) -> bool {
    let view = node.typed_view();
    if view.string("layoutMode") != Some("NONE") || !lays_nothing_out(node) {
        return false;
    }
    if super::style::asset_kind(snapshot, node).is_some() {
        return false;
    }
    only_visible_child(snapshot, node).is_some_and(|only| {
        only.typed_view()
            .value("constraints")
            .and_then(Value::as_object)
            .is_some_and(|constraints| {
                ["horizontal", "vertical"]
                    .iter()
                    .all(|axis| constraints.get(*axis).and_then(Value::as_str) == Some("CENTER"))
            })
    })
}

/// The plugin's `isFreelayout`: a frame in flow with no auto layout, whose
/// children sit where the designer left them.
fn lays_nothing_out(node: &RawNode) -> bool {
    let view = node.typed_view();
    view.string("layoutPositioning") == Some("AUTO")
        && view.number("width").is_some()
        && view.number("height").is_some()
        && view
            .value("inferredAutoLayout")
            .and_then(Value::as_object)
            .is_none()
        && !matches!(
            view.string("layoutMode"),
            Some("HORIZONTAL" | "VERTICAL" | "GRID")
        )
}

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
    let derived = derived_padding(snapshot, node);
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

/// Whether a node in flow is nonetheless placed by its parent, because the
/// parent lays nothing out.
///
/// A frame with no auto layout puts each child where the designer left it,
/// and the plugin's `canBeAbsolute` writes every such child at its
/// constraints — `pos="absolute"` with the edges it is pinned to — and gives
/// the frame `pos="relative"` to hold them. Here that was only done for a
/// child marked absolute, so the notice banner's title and its two logos, three
/// children of a free frame, were stacked in flow with no position at all.
///
/// Two cases are kept out. A frame whose single child's inset can be measured
/// is written with that inset as padding and the child in flow, which puts it
/// in the same place and lets it size the frame; and a frame whose single
/// child is centred is written as a `Center` with the child in flow, see
/// `centres_its_only_child`.
pub(crate) fn placed_by_a_free_layout(
    snapshot: &Snapshot,
    node: &RawNode,
    parent: Option<&RawNode>,
    is_page_root: bool,
) -> bool {
    let view = node.typed_view();
    let Some(parent) = parent else {
        return false;
    };
    // Whether the parent is itself placed by its own parent or pinned into
    // it says nothing about whether it lays its children out. The devup-ui
    // landing page's join-us panel holds a group of ten circles pinned to
    // the card at -277,-187; because the group is `ABSOLUTE`, its children
    // were read as being in flow, lost the coordinates they carry, stacked
    // from the group's corner and were clipped away - the arcs and both
    // badges were not drawn at all.
    //
    // This is only about the parent. `lays_nothing_out` also answers for a
    // node about itself, where being pinned does decide what it is, so it is
    // left alone.
    let parent_view = parent.typed_view();
    let parent_lays_nothing_out = matches!(
        parent_view.string("layoutPositioning"),
        Some("AUTO" | "ABSOLUTE")
    ) && parent_view.number("width").is_some()
        && parent_view.number("height").is_some()
        && parent_view
            .value("inferredAutoLayout")
            .and_then(Value::as_object)
            .is_none()
        && !matches!(
            parent_view.string("layoutMode"),
            Some("HORIZONTAL" | "VERTICAL" | "GRID")
        );
    !is_page_root
        && view.value("constraints").is_some()
        && parent_lays_nothing_out
        && derived_padding(snapshot, parent).is_none()
        && !centres_its_only_child(snapshot, parent)
}

fn push_absolute(
    snapshot: &Snapshot,
    node: &RawNode,
    parent: Option<&RawNode>,
    props: &mut Vec<Prop>,
    placed_by: Option<Box4>,
) {
    string_prop(props, "pos", "absolute");
    let view = node.typed_view();
    let Some(parent) = parent else {
        return;
    };
    // The box to place: the node's own, or the one handed in - an asset's
    // export.
    let mut own = Box4 {
        x: view.number("x").unwrap_or(0.0),
        y: view.number("y").unwrap_or(0.0),
        w: view.number("width").unwrap_or(0.0),
        h: view.number("height").unwrap_or(0.0),
    };
    // A group's children carry their `x` and `y` in the group's parent's
    // space, not the group's: the join-us panel's outermost circle, which is
    // exactly the group, reads `-277,-187` - the group's own place in the
    // card - where the group's own space would say `0,0`. Placed as read,
    // every circle sat 277px left and 187px high of where Figma draws it.
    // The absolute boxes settle it without depending on whose space a
    // number is in; an asset's export already went through them and landed
    // right.
    if parent.typed_view().node_type() == "GROUP" {
        match (layout_box(node), layout_box(parent)) {
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
    let placed = placed_by.unwrap_or(own);
    let parent = parent.typed_view();
    // A group has no constraints of its own; the plugin's `getPositionProps`
    // reads its first child's, and so does this. The report section's
    // illustration is a group pinned to the bottom of its frame through its
    // children, `bottom="-284.8px"`, where reading the group alone put it
    // at `top`.
    let constraints = view
        .value("constraints")
        .and_then(Value::as_object)
        .or_else(|| {
            view.child_ids()
                .next()
                .and_then(|child| snapshot.nodes.get(child))?
                .typed_view()
                .value("constraints")
                .and_then(Value::as_object)
        });
    let horizontal = constraints
        .and_then(|value| value.get("horizontal"))
        .and_then(Value::as_str)
        .unwrap_or("MIN");
    let vertical = constraints
        .and_then(|value| value.get("vertical"))
        .and_then(Value::as_str)
        .unwrap_or("MIN");
    let (x, y) = (placed.x, placed.y);
    match horizontal {
        "MAX" => string_prop(
            props,
            "right",
            px(parent.number("width").unwrap_or(0.0) - x - placed.w),
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
            px(parent.number("height").unwrap_or(0.0) - y - placed.h),
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
