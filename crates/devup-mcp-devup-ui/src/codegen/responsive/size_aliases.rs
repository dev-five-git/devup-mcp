/// Overlapping boxSize/w/h across frames must become independent physical
/// axes before breakpoint inheritance. A missing desktop h must not reset the
/// height supplied by desktop boxSize to CSS initial.
fn normalize_responsive_size(by_slot: &BySlot<Tree>) -> Option<BySlot<Tree>> {
    if by_slot.iter().flatten().count() < 2
        || by_slot.iter().flatten().any(|tree| tree.is_component)
        || !by_slot
            .iter()
            .flatten()
            .any(|tree| tree.props.contains_key("boxSize"))
        || !by_slot
            .iter()
            .flatten()
            .any(|tree| tree.props.contains_key("w") || tree.props.contains_key("h"))
    {
        return None;
    }
    Some(std::array::from_fn(|slot| {
        by_slot[slot].clone().map(|mut tree| {
            let original = tree.props.clone();
            for name in ["boxSize", "w", "h"] {
                tree.props.remove(name);
            }
            for axis in ["w", "h"] {
                let value = original
                    .get(axis)
                    .or_else(|| original.get("boxSize"))
                    .cloned()
                    .unwrap_or_else(|| "initial".into());
                tree.props.insert(axis.into(), value);
            }
            tree
        })
    }))
}
