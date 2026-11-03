/// A mobile gap shorthand followed by axis-specific desktop gaps cannot leave
/// a gap=initial reset: CSS shorthand declaration order can erase one desktop
/// axis. Resolve each captured frame into the two independent CSS axes first.
fn normalize_responsive_gap(by_slot: &BySlot<Tree>, snapshot: &Snapshot) -> Option<BySlot<Tree>> {
    if by_slot.iter().flatten().count() < 2
        || by_slot.iter().flatten().any(|tree| tree.is_component)
        || !by_slot.iter().flatten().any(|tree| tree.props.contains_key("gap"))
        || !by_slot.iter().flatten().any(|tree| ["rowGap", "columnGap", "colGap"].iter().any(|name| tree.props.contains_key(*name)))
    {
        return None;
    }
    // Do not manufacture known axes for an unreadable width. The existing
    // partial projection and its gaps remain available without this rewrite.
    for tree in by_slot.iter().flatten() {
        let node = snapshot.nodes.get(&tree.node_id)?;
        let view = node.typed_view();
        for field in ["itemSpacing", "counterAxisSpacing", "inferredAutoLayout", "layoutMode", "layoutWrap"] {
            if view.field_error(field).is_some() { return None; }
        }
        for field in ["itemSpacing", "counterAxisSpacing"] {
            if view.value(field).is_some_and(|value| value.as_f64().is_none()) { return None; }
        }
        if !["gap", "rowGap", "columnGap", "colGap"].iter().any(|name| tree.props.contains_key(*name)) {
            let spacing = view.number("itemSpacing").or_else(|| view.value("inferredAutoLayout")
                .and_then(|layout| layout.get("itemSpacing")).and_then(Value::as_f64));
            if spacing != Some(0.0) || view.number("counterAxisSpacing") != Some(0.0) { return None; }
        }
    }
    Some(std::array::from_fn(|slot| by_slot[slot].clone().map(|mut tree| {
        let original = tree.props.clone();
        for name in ["gap", "rowGap", "columnGap", "colGap"] { tree.props.remove(name); }
        for (axis, alias) in [("rowGap", "rowGap"), ("columnGap", "colGap")] {
            let value = original.get(axis).or_else(|| original.get(alias)).or_else(|| original.get("gap"))
                .cloned().unwrap_or_else(|| "initial".into());
            tree.props.insert(axis.into(), value);
        }
        tree
    })))
}
