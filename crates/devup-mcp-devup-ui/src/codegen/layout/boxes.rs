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

