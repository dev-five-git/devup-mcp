/// What a layout element does by itself besides its `display`.
fn implied_layout(component: &str) -> &'static [(&'static str, &'static str)] {
    match component {
        "Flex" => &[("flexDir", "row")],
        "VStack" => &[("flexDir", "column")],
        "Center" => &[("alignItems", "center"), ("justifyContent", "center")],
        _ => &[],
    }
}

/// The `display` a devup-ui element already carries by being itself.
fn implied_display(component: &str) -> Option<&'static str> {
    match component {
        "Flex" | "VStack" | "Center" => Some("flex"),
        "Grid" => Some("grid"),
        _ => None,
    }
}

/// What `display` an element goes back to when a wider width stops hiding it.
///
/// `merge_slots` clears a dropped layout prop to `"initial"`, which is right
/// for the other forty and wrong for this one. `initial` is the value the CSS
/// specification gives a property, not the value the element has: for `w` that
/// is `auto` and for `p` it is `0`, which are what "unset" should mean, but for
/// `display` it is `inline` whatever the element is. An element's own display
/// comes from the user-agent stylesheet instead, so a `Box` shown only from
/// tablet up would come back inline rather than block.
///
/// `revert` is the keyword that would mean what is wanted, and it cannot be
/// used either: it rolls the cascade back past the author origin, which is
/// where devup-ui puts `VStack`'s own `display: flex`. So the value is written
/// out, and `display` is never cleared to `initial`.
fn natural_display(component: &str) -> &'static str {
    match component {
        "Flex" | "VStack" | "Center" => "flex",
        "Grid" => "grid",
        // `Image` is an `img`, which is inline. Everything else devup-ui draws
        // is a `div`, and `Text` is a `p`.
        "Image" => "inline",
        _ => "block",
    }
}

/// Put back the `display` the element name stands for, before any merging.
///
/// The plugin keeps `display` in a node's props and picks the element from it;
/// this projection picks the element first and drops the value. That is the
/// same thing for one width, but not across several: a width where the node is
/// hidden would leave the others with nothing to go back to, and `merge_slots`
/// would clear them to `"initial"` — which for `display` is `inline`, not
/// `flex`. The node would stop being a flex container at every width it is
/// shown. Rendering drops the value again where the element implies it, so
/// nothing is written that was not written before.
fn restore_implied_display(tree: &mut Tree) {
    if let Some(display) = implied_display(&tree.component) {
        tree.props
            .entry("display".to_owned())
            .or_insert_with(|| display.to_owned());
    }
    for child in &mut tree.children {
        restore_implied_display(child);
    }
}

/// An omitted Figma MIN default is not an absent breakpoint. Restore only a
/// read-safe captured MIN (direct or inferred when the direct default was
/// omitted), leaving unknown reads unverified instead of clearing every gap.
fn normalize_source_main_alignment(by_slot: &BySlot<Tree>, snapshot: &Snapshot) -> Option<BySlot<Tree>> {
    if !by_slot.iter().flatten().any(|tree| tree.props.contains_key("justifyContent")) {
        return None;
    }
    let mut changed = false;
    let normalized = std::array::from_fn(|slot| by_slot[slot].clone().map(|mut tree| {
        if !tree.is_component && LAYOUT_ELEMENTS.contains(&tree.component.as_str())
            && !tree.props.contains_key("justifyContent")
            && let Some(node) = snapshot.nodes.get(&tree.node_id)
        {
            let view = node.typed_view();
            let primary = match view.value("primaryAxisAlignItems") {
                Some(value) => value.as_str(),
                None if view.field_error("inferredAutoLayout").is_none() => view.value("inferredAutoLayout")
                    .and_then(|layout| layout.get("primaryAxisAlignItems")).and_then(Value::as_str),
                None => None,
            };
            if primary == Some("MIN") && view.field_error("primaryAxisAlignItems").is_none()
                && view.field_error("layoutMode").is_none()
                && matches!(view.string("layoutMode"), Some("HORIZONTAL" | "VERTICAL"))
            {
                // CSS initial is normal, whose flex-layout distribution starts.
                tree.props.insert("justifyContent".into(), "initial".into());
                changed = true;
            }
        }
        tree
    }));
    changed.then_some(normalized)
}
