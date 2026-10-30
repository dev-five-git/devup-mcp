

/// Preserve source tracks, including content sizing. A flexible column in a
/// non-HUG grid has a zero minimum so replaced content cannot enlarge its share
/// through CSS's automatic min-content minimum. Rows and HUG grids retain their
/// content minimum. Count-only grids retain the equal-share fallback, explicitly
/// diagnosed as a policy choice rather than a recovered source measurement.
pub(super) fn grid_template(node: &RawNode, columns: bool) -> GridTemplate {
    // A codegen resource budget, not a Figma limit: unsupported input must be
    // diagnosed instead of allocating a huge list or emitting a clamped grid.
    const MAX_TRACKS: usize = 4096;
    let view = node.typed_view();
    let (sizes, count, sizing) = if columns {
        (
            "gridColumnSizes",
            "gridColumnCount",
            "layoutSizingHorizontal",
        )
    } else {
        ("gridRowSizes", "gridRowCount", "layoutSizingVertical")
    };
    let hug = view.string(sizing) == Some("HUG");
    let flexible = |value| {
        let fraction = format!("{}fr", format_number(value));
        if columns && !hug {
            format!("minmax(0, {fraction})")
        } else {
            fraction
        }
    };
    let mut unverified = Vec::new();
    let count_value = view.value(count);
    let count = view.number(count);
    let sizes = view.value(sizes).and_then(Value::as_array);
    if count.is_some_and(|count| count > MAX_TRACKS as f64) {
        unverified.push("track-count-exceeds-codegen-budget");
    }
    if sizes.is_some_and(|sizes| sizes.len() > MAX_TRACKS) {
        unverified.push("track-list-exceeds-codegen-budget");
    }
    if !unverified.is_empty() {
        return GridTemplate {
            css: None,
            unverified,
        };
    }
    let valid_count = count.filter(|count| *count >= 1.0 && count.fract() == 0.0);
    let tracks = if let Some(sizes) = sizes.filter(|sizes| !sizes.is_empty()) {
        if count_value.is_some_and(|v| !v.is_null()) && valid_count != Some(sizes.len() as f64) {
            unverified.push("track-count-mismatch");
        }
        sizes
            .iter()
            .map(|track| {
                let value = track.get("value").and_then(Value::as_f64);
                match track.get("type").and_then(Value::as_str) {
                    Some("FIXED") if value.is_some_and(|v| v >= 0.0) => px(value.unwrap()),
                    Some("HUG") => "fit-content(100%)".to_owned(),
                    Some("AUTO") => "auto".to_owned(),
                    Some("FLEX")
                        if value.is_some_and(|v| v >= 0.0)
                            || track.get("value").is_none_or(Value::is_null) =>
                    {
                        flexible(value.unwrap_or(1.0))
                    }
                    _ => {
                        // Unknown track kinds or invalid fixed sizes are not
                        // evidence for either a pixel size or a flexible share.
                        unverified.push("unknown-or-invalid-track-size");
                        "auto".to_owned()
                    }
                }
            })
            .collect::<Vec<_>>()
    } else {
        unverified.push("track-sizes-not-collected");
        if valid_count.is_none() {
            unverified.push("track-count-not-collected-or-invalid");
        }
        // Count-only input needs no allocation proportional to an untrusted
        // track count, and an absent count must not emit an empty CSS value.
        let css = valid_count.map(|count| {
            let track = if hug {
                "auto".to_owned()
            } else {
                flexible(1.0)
            };
            format!("repeat({}, {track})", format_number(count))
        });
        return GridTemplate { css, unverified };
    };
    unverified.sort_unstable();
    unverified.dedup();
    let css = if tracks.is_empty() {
        None
    } else if tracks.iter().all(|track| track == &tracks[0]) {
        Some(format!("repeat({}, {})", tracks.len(), tracks[0]))
    } else {
        Some(tracks.join(" "))
    };
    GridTemplate { css, unverified }
}

/// Figma centers a lone in-flow child for SPACE_BETWEEN; CSS flexbox starts it.
/// Hidden and absolutely positioned children do not participate in distribution.
pub(crate) fn centers_lone_space_between_child(snapshot: &Snapshot, node: &RawNode) -> bool {
    let view = node.typed_view();
    matches!(view.string("layoutMode"), Some("HORIZONTAL" | "VERTICAL"))
        && view.string("primaryAxisAlignItems") == Some("SPACE_BETWEEN")
        && view
            .child_ids()
            .filter_map(|id| snapshot.nodes.get(id))
            .filter(|child| {
                let child = child.typed_view();
                child.bool("visible") != Some(false)
                    && child.string("layoutPositioning") != Some("ABSOLUTE")
            })
            .count()
            == 1
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
        for (columns, prop) in [(true, "gridTemplateColumns"), (false, "gridTemplateRows")] {
            if let Some(css) = grid_template(node, columns).css {
                string_prop(props, prop, css);
            }
        }
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
        Some("SPACE_BETWEEN") if centers_lone_space_between_child(snapshot, node) => Some("center"),
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
    let [
        Some(mut top),
        Some(mut right),
        Some(mut bottom),
        Some(mut left),
    ] = [
        get("paddingTop"),
        get("paddingRight"),
        get("paddingBottom"),
        get("paddingLeft"),
    ]
    else {
        return;
    };
    // An inside stroke is painted over the padding: a card 20px in from its
    // edge with a 1px stroke inside is still 20px in, stroke and all, and
    // its height is the content plus 40. CSS's border is added around the
    // padding instead, so a hugging card came out 2px taller - the landing
    // page's four feature cards put the join-us panel 8px down - and on a
    // fixed axis the content sat 1px further in. Taking the stroke out of
    // the padding puts the content where Figma has it on every axis: it
    // starts `p` in and, hugging, the box is content plus `2p`.
    if view.string("strokeAlign").unwrap_or("INSIDE") == "INSIDE"
        && !super::style::inside_stroke_uses_outline(&view)
        && view.node_type() != "LINE"
        && let Some(weight) = view.number("strokeWeight").filter(|weight| *weight > 0.0)
        && view
            .value("strokes")
            .and_then(Value::as_array)
            .is_some_and(|strokes| {
                strokes.iter().any(|paint| {
                    paint.get("visible").and_then(Value::as_bool) != Some(false)
                        && paint.get("type").and_then(Value::as_str) == Some("SOLID")
                })
            })
    {
        top = (top - weight).max(0.0);
        bottom = (bottom - weight).max(0.0);
        left = (left - weight).max(0.0);
        right = (right - weight).max(0.0);
    }
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

/// Whether `child_id` is a positioned child of `parent` that Figma draws
/// under everything else in it: nothing in flow comes before it, and
/// something in flow comes after. CSS paints a positioned element after every
/// in-flow sibling whatever the order, so such a child has to be sent behind
/// on purpose.
///
/// `zIndex="-1"` sends it behind *every* in-flow sibling, not only the later
/// ones, so it is only right when there is nothing earlier to stay above.
/// The notice page's header is pinned second, after its banner: sent behind,
/// it vanished under the banner it is meant to sit on. It is left where CSS
/// puts it, above all of them, which nothing there overlaps anyway.
fn sits_behind_in_flow_siblings(snapshot: &Snapshot, parent: &RawNode, child_id: &str) -> bool {
    let ids = parent.typed_view().child_ids().collect::<Vec<_>>();
    let Some(index) = ids.iter().position(|id| *id == child_id) else {
        return false;
    };
    let positioned = |node: &RawNode| {
        node.typed_view().string("layoutPositioning") == Some("ABSOLUTE")
            || placed_by_a_free_layout(snapshot, node, Some(parent), false)
    };
    let in_flow =
        |node: &RawNode| node.typed_view().bool("visible") != Some(false) && !positioned(node);
    let Some(child) = snapshot.nodes.get(child_id) else {
        return false;
    };
    if !positioned(child) {
        return false;
    }
    let earlier_in_flow = ids[..index]
        .iter()
        .filter_map(|id| snapshot.nodes.get(*id))
        .any(in_flow);
    let later_in_flow = ids[index + 1..]
        .iter()
        .filter_map(|id| snapshot.nodes.get(*id))
        .any(in_flow);
    !earlier_in_flow && later_in_flow
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

/// Places a positioned node, and returns the anchor a width policy chose for
/// it, if one was given.
fn push_absolute(
    snapshot: &Snapshot,
    node: &RawNode,
    parent: Option<&RawNode>,
    props: &mut Vec<Prop>,
    placed_by: Option<Box4>,
    policy: Option<&WidthPolicy>,
) -> Option<Anchor> {
    let parent_node = parent?;
    string_prop(props, "pos", "absolute");
    let view = node.typed_view();
    // The box to place: the node's own, or the one handed in - an asset's
    // export. A group's children carry their `x` and `y` in the group's
    // parent's space, not the group's: the join-us panel's outermost circle,
    // which is exactly the group, reads `-277,-187` - the group's own place in
    // the card - where the group's own space would say `0,0`. Placed as read,
    // every circle sat 277px left and 187px high of where Figma draws it.
    // `placed_box` settles it from the absolute boxes, without depending on
    // whose space a number is in.
    let placed = super::width::placed_box(node, parent_node, placed_by);
    let parent = parent_node.typed_view();
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
    let anchor =
        policy.map(|policy| super::width::effective_anchor(snapshot, node, parent_node, policy).0);
    match (anchor, horizontal) {
        (Some(anchor), _) => super::width::push_horizontal(
            props,
            anchor,
            placed,
            parent.number("width").unwrap_or(0.0),
        ),
        (None, "MAX") => string_prop(
            props,
            "right",
            px(parent.number("width").unwrap_or(0.0) - x - placed.w),
        ),
        (None, "CENTER") => {
            string_prop(props, "left", "50%");
            string_prop(props, "transform", "translateX(-50%)");
        }
        (None, _) => string_prop(props, "left", px(x)),
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
    anchor
}

pub(super) fn string_prop(props: &mut Vec<Prop>, name: &str, value: impl Into<String>) {
    let value = PropValue::String(value.into());
    if let Some((_, existing)) = props.iter_mut().find(|(existing, _)| existing == name) {
        *existing = value;
    } else {
        props.push((name.to_owned(), value));
    }
}

pub(crate) fn px(value: f64) -> String {
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

/// Dimensions whose child-based HUG sizing is destroyed by mask projection.
/// Render bounds alone cannot establish layout size (effects may enlarge them).
pub(crate) fn folded_mask_dimensions(snapshot: &Snapshot, node: &RawNode) -> Option<(f64, f64)> {
    let view = node.typed_view();
    if super::style::asset_kind(snapshot, node) != Some(super::style::AssetKind::SvgMask)
        || view.child_ids().next().is_none()
        // Rotated exports need a separate transform proof.
        || view.number("rotation").is_some_and(|rotation| rotation.abs() > 0.01)
        || !(view.string("layoutSizingHorizontal") == Some("HUG")
            || view.string("layoutSizingVertical") == Some("HUG"))
    {
        return None;
    }
    if export_offset(node).is_some() {
        // In-flow HUG/FIXED axes retain the measured layout box. The export's
        // larger painted bounds are independently preserved by maskSize and
        // maskPos. A FILL axis would scale that box while leaving pixel mask
        // offsets unchanged; positioned assets instead use the export box.
        let parent = view
            .string("parentId")
            .and_then(|id| snapshot.nodes.get(id));
        if !matches!(view.string("layoutSizingHorizontal"), Some("HUG" | "FIXED"))
            || !matches!(view.string("layoutSizingVertical"), Some("HUG" | "FIXED"))
            || view.string("layoutPositioning") == Some("ABSOLUTE")
            || placed_by_a_free_layout(snapshot, node, parent, false)
        {
            return None;
        }
    }
    let measured = |axis| {
        view.number(axis)
            .or_else(|| view.value("absoluteBoundingBox")?.get(axis)?.as_f64())
            .filter(|v| v.is_finite() && *v > 0.0)
    };
    Some((measured("width")?, measured("height")?))
}
