/// Explicit empty paint differs from an uncollected/unreadable field. The fast
/// collector omits empty arrays after reading the complete manifest; native
/// layout nodes with captured dimensions can therefore have no paint field.
/// Component calls, missing nodes and failed/marked reads provide no such proof.
fn source_paint_is_empty(node: &RawNode, field: &str) -> bool {
    if !matches!(node.node_type.as_str(), "FRAME" | "INSTANCE" | "COMPONENT" | "COMPONENT_SET")
        || node.field_errors.contains_key(field)
        || !["width", "height"].iter().all(|name| {
            node.fields.get(*name).and_then(Value::as_f64).is_some()
        })
    {
        return false;
    }
    match node.fields.get(field) {
        None => true, // Manifest-backed fast-snapshot empty-array omission.
        Some(Value::Array(paints)) => paints.iter().all(|paint| {
            paint.is_object()
                && paint.get("visible").and_then(Value::as_bool) == Some(false)
        }),
        Some(_) => false, // Null, unsupported markers and malformed reads are unknown.
    }
}

fn paint_source_fields(prop: &str) -> &'static [&'static str] {
    match prop {
        "bg" | "bgImage" | "bgColor" | "background" | "backgroundImage" => &["fills"],
        "border" | "borderTop" | "borderRight" | "borderBottom" | "borderLeft"
        | "outline" | "outlineOffset" => &["strokes"],
        // Outside strokes can also produce a shadow, so both sources must be empty.
        "boxShadow" => &["effects", "strokes"],
        "filter" | "backdropFilter" => &["effects"],
        _ => &[],
    }
}

/// A mixed stroke still paints some edges. An explicitly captured zero on
/// another edge proves only that side is absent, not that all strokes are.
fn source_stroke_edge_is_zero(node: &RawNode, prop: &str) -> bool {
    let field = match prop {
        "borderTop" => "strokeTopWeight",
        "borderRight" => "strokeRightWeight",
        "borderBottom" => "strokeBottomWeight",
        "borderLeft" => "strokeLeftWeight",
        _ => return false,
    };
    !node.field_errors.contains_key(field)
        && !node.field_errors.contains_key("strokes")
        && node.fields.get(field).and_then(Value::as_f64) == Some(0.0)
        && node.fields.get("strokes").and_then(Value::as_array).is_some_and(|paints| {
            paints.iter().all(|paint| paint.get("type").and_then(Value::as_str).is_some())
        })
}

fn normalize_source_paint_defaults(
    by_slot: &BySlot<Tree>,
    snapshot: &Snapshot,
) -> Option<BySlot<Tree>> {
    let keys = by_slot.iter().flatten().filter(|tree| !tree.is_component)
        .flat_map(|tree| tree.props.keys())
        .filter(|key| !paint_source_fields(key).is_empty())
        .collect::<BTreeSet<_>>();
    let mut changed = false;
    let normalized = std::array::from_fn(|slot| {
        by_slot[slot].clone().map(|mut tree| {
            if !tree.is_component && LAYOUT_ELEMENTS.contains(&tree.component.as_str())
                && let Some(node) = snapshot.nodes.get(&tree.node_id)
            {
                for key in &keys {
                    if !tree.props.contains_key(*key)
                        && (paint_source_fields(key).iter().all(|field| source_paint_is_empty(node, field))
                            || (!tree.props.contains_key("border") && source_stroke_edge_is_zero(node, key)))
                    {
                        tree.props.insert((*key).clone(), "initial".into());
                        changed = true;
                    }
                }
            }
            tree
        })
    });
    changed.then_some(normalized)
}
