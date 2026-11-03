fn border_origin_offsets(
    snapshot: &Snapshot,
    node: &RawNode,
    parent: &RawNode,
) -> Option<(f64, f64, f64, f64)> {
    let view = node.typed_view();
    let p = parent.typed_view();
    if view.string("layoutPositioning") != Some("ABSOLUTE")
        || view.bool("visible") == Some(false)
        || p.bool("visible") == Some(false)
        || !matches!(
            p.string("layoutMode"),
            Some("HORIZONTAL" | "VERTICAL" | "GRID")
        )
        || super::style::asset_kind(snapshot, parent).is_some()
        || super::style::asset_kind(snapshot, node).is_some()
        || [
            "constraints",
            "relativeTransform",
            "absoluteBoundingBox",
            "layoutPositioning",
            "rotation",
            "visible",
        ]
        .iter()
        .any(|field| view.field_error(field).is_some())
        || [
            "relativeTransform",
            "absoluteBoundingBox",
            "layoutMode",
            "rotation",
            "visible",
        ]
        .iter()
        .any(|field| p.field_error(field).is_some())
    {
        return None;
    }
    for v in [&view, &p] {
        let transform = v.value("relativeTransform")?.as_array()?;
        if transform.len() != 2
            || transform
                .iter()
                .any(|row| row.as_array().is_none_or(|row| row.len() != 3))
            || v.value("rotation")
                .is_some_and(|value| value.as_f64() != Some(0.0))
        {
            return None;
        }
        if transform[0][0].as_f64() != Some(1.0)
            || transform[0][1].as_f64() != Some(0.0)
            || transform[1][0].as_f64() != Some(0.0)
            || transform[1][1].as_f64() != Some(1.0)
        {
            return None;
        }
    }
    let constraints = view.value("constraints")?;
    if constraints["horizontal"] != "MIN" || constraints["vertical"] != "MIN" {
        return None;
    }
    let (source, parent_box) = (
        box4(view.value("absoluteBoundingBox"))?,
        box4(p.value("absoluteBoundingBox"))?,
    );
    let (x, y) = (source.x - parent_box.x, source.y - parent_box.y);
    let (left, top) = super::style::emitted_leading_border(parent)?;
    (x.is_finite() && y.is_finite()).then_some((x, y, left, top))
}

fn correct_absolute_border_origin(
    snapshot: &Snapshot,
    node: &RawNode,
    parent: &RawNode,
    props: &mut [Prop],
) {
    let Some((x, y, left, top)) = border_origin_offsets(snapshot, node, parent) else {
        return;
    };
    for (name, source, border) in [("left", x, left), ("top", y, top)] {
        if border > 0.0
            && let Some((_, PropValue::String(value))) =
                props.iter_mut().find(|(key, _)| key == name)
            && *value == px(source)
        {
            *value = px(source - border);
        }
    }
}

pub(crate) fn absolute_border_origin_calculation(
    snapshot: &Snapshot,
    node_id: &str,
    generated: &str,
) -> Option<String> {
    let node = snapshot.nodes.get(node_id)?;
    let parent = snapshot.nodes.get(node.typed_view().string("parentId")?)?;
    let (x, y, left, top) = border_origin_offsets(snapshot, node, parent)?;
    for (name, source, border) in [("left", x, left), ("top", y, top)] {
        if border > 0.0 && generated == format!("{name}=\"{}\"", px(source - border)) {
            return Some(format!(
                "CSS absolute border-origin conversion: captured MIN {name} offset {} minus emitted native parent {} leading border {} = {}; CSS containing-block border origin adds that edge back. Source parent {}; verification labels are unchanged.",
                px(source),
                parent.id,
                px(border),
                px(source - border),
                parent.id
            ));
        }
    }
    None
}
