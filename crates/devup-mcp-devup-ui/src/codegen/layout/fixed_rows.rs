pub(super) fn wrapping_row(snapshot: &Snapshot, node: &RawNode) -> bool {
    let view = node.typed_view();
    let name = view.name().unwrap_or_default().to_ascii_lowercase();
    if super::responsive::breakpoints(snapshot).len() > 1
        || view.string("layoutMode") != Some("HORIZONTAL")
        || view.string("layoutWrap") == Some("WRAP")
        || ["header", "nav", "gnb"].iter().any(|word| name.contains(word)) { return false; }
    let children = view.child_ids().filter_map(|id| snapshot.nodes.get(id))
        .filter(|child| child.typed_view().bool("visible") != Some(false)
            && child.typed_view().string("layoutPositioning") != Some("ABSOLUTE"))
        .collect::<Vec<_>>();
    children.len() >= 2
        && children.iter().all(|child| child.typed_view().string("layoutSizingHorizontal") == Some("FIXED") && child.typed_view().number("width").is_some())
        && children.iter().filter_map(|child| child.typed_view().number("width")).sum::<f64>()
            + view.number("itemSpacing").unwrap_or(0.0) * (children.len()-1) as f64 > 390.0
}

pub(super) fn in_wrapping_row(snapshot: &Snapshot, node: &RawNode) -> bool {
    let mut current = node;
    for _ in 0..16 {
        if wrapping_row(snapshot,current) { return true; }
        let Some(parent) = snapshot.nodes.values().find(|parent| parent.typed_view().child_ids().any(|id| id == current.id)) else {break};
        current = parent;
    }
    false
}

pub(super) fn fixed_row_props(snapshot: &Snapshot, node: &RawNode, props: &mut Vec<Prop>) {
    if wrapping_row(snapshot,node) {
        string_prop(props,"flexWrap","wrap");
        string_prop(props,"w","100%");
        string_prop(props,"h","auto");
        if let Some(width) = node.typed_view().number("width") { string_prop(props,"maxW",px(width)); }
    } else if in_wrapping_row(snapshot,node)
        && node.typed_view().string("layoutPositioning") != Some("ABSOLUTE") {
        string_prop(props,"maxW","100%");
        if snapshot.nodes.values().any(|parent| wrapping_row(snapshot,parent) && parent.typed_view().child_ids().any(|id| id == node.id)) {
            string_prop(props,"flexShrink","0");
        }
    }
}
