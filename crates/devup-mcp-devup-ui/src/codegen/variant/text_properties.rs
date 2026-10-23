fn bind_text_properties(
    snapshot: &Snapshot,
    set: &RawNode,
    tree: &mut Tree,
) -> BTreeMap<String, String> {
    let properties = set.typed_view()
        .value("componentPropertyDefinitions")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(_, definition)| definition["type"] == "TEXT")
        .map(|(name, definition)| (
            name.clone(),
            (instance_property_name(name.split('#').next().unwrap_or(name)), definition["defaultValue"].to_string()),
        ))
        .collect::<BTreeMap<_, _>>();
    let mut pending = vec![tree];
    while let Some(node) = pending.pop() {
        if let Some((name, _)) = snapshot.nodes.get(&node.node_id)
            .and_then(|raw| raw.typed_view().value("componentPropertyReferences"))
            .and_then(|references| references.get("characters"))
            .and_then(Value::as_str)
            .and_then(|reference| properties.get(reference))
        {
            node.content = Some(format!("{{{name}}}"));
        }
        pending.extend(node.children.iter_mut());
    }
    properties.into_values().collect()
}
