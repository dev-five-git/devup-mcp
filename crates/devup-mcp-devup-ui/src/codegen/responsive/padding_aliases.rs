/// Devup's responsive shorthand/longhand extraction can discard a narrower
/// side when a wider breakpoint uses an overlapping shorthand. The layout
/// writer emits scalar uniform padding props, so expand only families with
/// different overlapping aliases into their independent physical sides.
fn normalize_responsive_padding(by_slot: &BySlot<Tree>) -> Option<BySlot<Tree>> {
    if by_slot.iter().flatten().count() < 2
        || by_slot.iter().flatten().any(|tree| tree.is_component)
    {
        return None;
    }
    let aliases = [
        ("p", 15_u8),
        ("px", 3),
        ("py", 12),
        ("pl", 1),
        ("pr", 2),
        ("pt", 4),
        ("pb", 8),
    ];
    let present = aliases
        .iter()
        .filter(|(name, _)| {
            by_slot
                .iter()
                .flatten()
                .any(|tree| tree.props.contains_key(*name))
        })
        .collect::<Vec<_>>();
    if !present
        .iter()
        .enumerate()
        .any(|(index, (left, left_sides))| {
            present[index + 1..]
                .iter()
                .any(|(right, right_sides)| left != right && left_sides & right_sides != 0)
        })
    {
        return None;
    }
    let affected = present.iter().fold(0_u8, |sides, (_, mask)| sides | mask);
    Some(std::array::from_fn(|slot| {
        by_slot[slot].clone().map(|mut tree| {
            let original = tree.props.clone();
            for (name, _) in aliases {
                tree.props.remove(name);
            }
            for (side, axis, mask) in [
                ("pl", "px", 1_u8),
                ("pr", "px", 2),
                ("pt", "py", 4),
                ("pb", "py", 8),
            ] {
                if affected & mask == 0 {
                    continue;
                }
                let value = original
                    .get(side)
                    .or_else(|| original.get(axis))
                    .or_else(|| original.get("p"))
                    .cloned()
                    .unwrap_or_else(|| "initial".to_owned());
                tree.props.insert(side.to_owned(), value);
            }
            tree
        })
    }))
}
