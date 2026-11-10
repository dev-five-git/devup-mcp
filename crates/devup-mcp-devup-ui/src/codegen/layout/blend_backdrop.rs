// A blended decoration composites against whatever its stacking context has
// painted so far. Figma blends a layer with everything drawn beneath it on the
// canvas, so a `multiply` blob over a purple gradient comes out deep purple.
//
// The negative-z policy in `push_layout_props` gives the parent of a
// sent-behind child `zIndex="0"`, opening a stacking context there. When that
// parent is a transparent wrapper inside the element that paints the
// gradient, the context holds nothing beneath the decoration: `multiply`
// composites against transparency, and the Prime SEO-GEO contact banner
// (instance 32:1929) drew white blobs (224,224,224) where Figma has purple
// (77,44,208). Devup UI emits every property faithfully; the context was on
// the wrong element.
//
// So the context moves up to the nearest ancestor that paints the backdrop,
// across wrappers that paint nothing and open no context of their own. The
// decoration still sits above that ancestor's background and below the
// content, which is the containment the negative-z policy guarantees.

/// The node is blended with what is under it, not drawn over it.
fn blends_with_backdrop(view: &TypedNode<'_>) -> bool {
    view.string("blendMode")
        .and_then(super::style::blend_mode)
        .is_some()
}

fn visible_paints(view: &TypedNode<'_>, field: &str) -> bool {
    view.value(field).and_then(Value::as_array).is_some_and(|paints| {
        paints.iter().any(|paint| {
            paint.get("visible").and_then(Value::as_bool) != Some(false)
                && paint.get("opacity").and_then(Value::as_f64) != Some(0.0)
        })
    })
}

/// The node paints a backdrop for its descendants: a visible fill.
fn paints_backdrop(snapshot: &Snapshot, node: &RawNode) -> bool {
    super::style::asset_kind(snapshot, node).is_none() && visible_paints(&node.typed_view(), "fills")
}

/// A wrapper that paints nothing and opens no stacking context: no fill,
/// stroke or effect, full opacity, normal or pass-through blending, no
/// rotation, and not itself sent behind with a negative z-index. Blending
/// passes through such a wrapper in Figma and must pass through it in CSS.
fn passes_blending_through(snapshot: &Snapshot, node: &RawNode, parent: Option<&RawNode>) -> bool {
    let view = node.typed_view();
    super::style::asset_kind(snapshot, node).is_none()
        && !visible_paints(&view, "fills")
        && !visible_paints(&view, "strokes")
        && !visible_paints(&view, "effects")
        && view.number("opacity").is_none_or(|opacity| opacity >= 0.999)
        && !blends_with_backdrop(&view)
        && view.bool("isMask") != Some(true)
        && view
            .number("rotation")
            .is_none_or(|rotation| rotation.abs() <= 0.01)
        && !parent.is_some_and(|parent| {
            holds_positioned_children(snapshot, parent)
                && sits_behind_in_flow_siblings(snapshot, parent, &node.id)
        })
}

/// Nothing in `parent` is drawn before `child`. Lifting the context paints
/// the decoration under all of the ancestor's content, so anything Figma
/// draws earlier than it would wrongly end up above it.
fn first_visible_child(snapshot: &Snapshot, parent: &RawNode, child_id: &str) -> bool {
    parent
        .typed_view()
        .child_ids()
        .find(|id| {
            snapshot
                .nodes
                .get(*id)
                .is_some_and(|node| node.typed_view().bool("visible") != Some(false))
        })
        == Some(child_id)
}

fn has_blended_child_behind(snapshot: &Snapshot, node: &RawNode) -> bool {
    holds_positioned_children(snapshot, node)
        && node.typed_view().child_ids().any(|child| {
            sits_behind_in_flow_siblings(snapshot, node, child)
                && snapshot
                    .nodes
                    .get(child)
                    .is_some_and(|child| blends_with_backdrop(&child.typed_view()))
        })
}

fn parent_in_snapshot<'a>(snapshot: &'a Snapshot, node: &RawNode) -> Option<&'a RawNode> {
    snapshot
        .nodes
        .values()
        .find(|candidate| candidate.typed_view().child_ids().any(|child| child == node.id))
}

/// The ancestor that should hold the stacking context of `node`'s blended
/// negative-z children instead of `node`, if `node` is a transparent wrapper
/// below the element that paints their backdrop.
fn blend_context_host<'a>(snapshot: &'a Snapshot, node: &RawNode) -> Option<&'a RawNode> {
    if !has_blended_child_behind(snapshot, node) {
        return None;
    }
    let mut current = snapshot.nodes.get(&node.id)?;
    let mut parent = parent_in_snapshot(snapshot, current)?;
    if !passes_blending_through(snapshot, current, Some(parent)) {
        return None;
    }
    loop {
        if !first_visible_child(snapshot, parent, &current.id) {
            return None;
        }
        if paints_backdrop(snapshot, parent) {
            return Some(parent);
        }
        let grandparent = parent_in_snapshot(snapshot, parent)?;
        if !passes_blending_through(snapshot, parent, Some(grandparent)) {
            return None;
        }
        current = parent;
        parent = grandparent;
    }
}

/// Whether `node` holds the stacking context lifted from a transparent
/// wrapper below it. Walks down through wrappers only, so `node` is the
/// nearest painting ancestor of whatever it finds.
fn hosts_lifted_blend_context(snapshot: &Snapshot, node: &RawNode) -> bool {
    if !paints_backdrop(snapshot, node) {
        return false;
    }
    let mut stack = vec![node];
    while let Some(parent) = stack.pop() {
        let Some(first) = parent
            .typed_view()
            .child_ids()
            .filter_map(|id| snapshot.nodes.get(id))
            .find(|child| child.typed_view().bool("visible") != Some(false))
        else {
            continue;
        };
        if !passes_blending_through(snapshot, first, Some(parent)) {
            continue;
        }
        if has_blended_child_behind(snapshot, first) {
            return true;
        }
        stack.push(first);
    }
    false
}
