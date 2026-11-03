// Inline responsive projection must resolve the same captured boolean overrides
// as the ordinary renderer. A component master keeps its symbolic properties.
fn captured_instance_visibility(snapshot: &Snapshot, node: &RawNode) -> Option<bool> {
    let reference = node
        .typed_view()
        .value("componentPropertyReferences")?
        .get("visible")?
        .as_str()?;
    let mut current = node.typed_view().string("parentId").map(str::to_owned);
    let mut seen = BTreeSet::new();
    while let Some(id) = current {
        if !seen.insert(id.clone()) {
            break;
        }
        let ancestor = snapshot.nodes.get(&id)?;
        let view = ancestor.typed_view();
        if ancestor.node_type == "INSTANCE"
            && let Some(property) = view
                .value("componentProperties")
                .and_then(|values| values.get(reference))
            && property["type"] == "BOOLEAN"
            && let Some(value) = property["value"].as_bool()
        {
            return Some(value);
        }
        current = view.string("parentId").map(str::to_owned);
    }
    None
}

fn concretely_hidden_tree(node: &RawNode) -> Tree {
    Tree {
        node_id: node.id.clone(),
        node_name: node.typed_view().name().unwrap_or_default().into(),
        source_node_ids: BTreeSet::from([node.id.clone()]),
        component: "Box".into(),
        props: BTreeMap::from([("display".into(), "none".into())]),
        children: Vec::new(),
        content: None,
        is_component: false,
        visible_when: None,
        drawn_when: Vec::new(),
        leading_comment: None,
        slots: Vec::new(),
        scalar_sources: Vec::new(),
    }
}
