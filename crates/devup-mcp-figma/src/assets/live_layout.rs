/// A one-child layout is not a scalable picture when its child has independent
/// captured responsive sizing. Keep that existing Figma container in the DOM so
/// a FILL header row cannot scale its fixed-size logo as an SVG viewport.
pub fn single_child_layout_must_remain_live(snapshot: &Snapshot, node: &RawNode) -> bool {
    let view = node.typed_view();
    if !matches!(node.node_type.as_str(), "FRAME" | "INSTANCE" | "COMPONENT")
        || view.bool("isAsset") == Some(true)
        || !matches!(
            view.string("layoutMode").or_else(|| view
                .value("inferredAutoLayout")?
                .get("layoutMode")?
                .as_str()),
            Some("HORIZONTAL" | "VERTICAL")
        )
    {
        return false;
    }
    let children = view
        .child_ids()
        .filter_map(|id| snapshot.nodes.get(id))
        .filter(|child| child.typed_view().bool("visible") != Some(false))
        .collect::<Vec<_>>();
    let [child] = children.as_slice() else {
        return false;
    };
    let cv = child.typed_view();
    if cv.string("layoutPositioning") == Some("ABSOLUTE") {
        return false;
    }
    [
        ("layoutSizingHorizontal", "layoutSizingHorizontal"),
        ("layoutSizingVertical", "layoutSizingVertical"),
    ]
    .iter()
    .any(|(outer, inner)| view.string(outer) == Some("FILL") && cv.string(inner) == Some("FIXED"))
}
