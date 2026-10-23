/// Marks the page root, its top-level children (the sections), their
/// children and every asset with `data-figma-node-id`, so the runtime audit captures a section
/// or an asset by the node it came from. Only when `figmaNodeIds` is true.
fn annotate_node_ids(
    output: &mut devup_mcp_devup_ui::codegen::CodegenOutput,
    snapshot: &devup_mcp_figma::Snapshot,
    root_id: &str,
) -> Value {
    let sections = snapshot
        .nodes
        .get(root_id)
        .map(|node| {
            node.typed_view()
                .child_ids()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    // A section's own children (the cards of a card row) too, so their
    // order and position are checked against the frame.
    let parts = sections
        .iter()
        .filter_map(|id| snapshot.nodes.get(id))
        .flat_map(|node| {
            node.typed_view()
                .child_ids()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let assets = output
        .source_map
        .entries
        .iter()
        .filter(|entry| entry.asset_id.is_some())
        .filter_map(|entry| entry.node_id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let wanted = std::iter::once(root_id.to_owned())
        .chain(sections.iter().cloned())
        .chain(parts.iter().cloned())
        .chain(assets.iter().cloned())
        .collect();
    let written = devup_mcp_devup_ui::codegen::node_ids::annotate_node_ids(output, &wanted);
    json!({
        "attribute": devup_mcp_devup_ui::codegen::node_ids::NODE_ID_ATTRIBUTE,
        "root": written.iter().any(|id| id == root_id).then_some(root_id),
        "sections": sections.iter().filter(|id| written.contains(id)).collect::<Vec<_>>(),
        "parts": parts.iter().filter(|id| written.contains(id)).collect::<Vec<_>>(),
        "assets": assets.iter().filter(|id| written.contains(id)).collect::<Vec<_>>(),
        "note": include_str!("../text/projection-annotate_node_ids-b03e6d56.md"),
    })
}

fn generate_collected_component(
    payload: &CollectedPayload,
    node_id: &str,
    options: &CodegenOptions,
) -> Result<devup_mcp_devup_ui::codegen::CodegenOutput, DevupError> {
    let snapshot = &payload.snapshot;
    let root = snapshot.nodes.get(node_id);
    let master = root.filter(|node| node.node_type == "INSTANCE").and_then(|node| {
        let view = node.typed_view();
        view.string("mainComponentId")
            .or_else(|| view.string("componentId"))
            .or_else(|| view.string("mainComponent"))
            .or_else(|| view.value("mainComponent").and_then(|value| value.get("id")).and_then(Value::as_str))
    });
    let target = master.and_then(|id| snapshot.nodes.get(id)).or(root);
    let target = target.and_then(|node| {
        let parent = node.typed_view().string("parentId")
            .and_then(|id| snapshot.nodes.get(id))
            .filter(|parent| parent.node_type == "COMPONENT_SET");
        parent.or(Some(node))
    });
    if !options.inline_instances
        && !target.is_some_and(|node| matches!(node.node_type.as_str(), "COMPONENT" | "COMPONENT_SET"))
    {
        let mut pending = vec![node_id];
        let mut visited = std::collections::BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            let Some(node) = snapshot.nodes.get(id) else {
                continue;
            };
            let view = node.typed_view();
            if view.bool("visible") == Some(false) {
                continue;
            }
            if node.node_type == "INSTANCE"
                && let Some(component_id) = view.string("mainComponentId")
                    .or_else(|| view.string("componentId"))
                    .or_else(|| view.string("mainComponent"))
                    .or_else(|| view.value("mainComponent").and_then(|value| value.get("id")).and_then(Value::as_str))
                && !snapshot.nodes.contains_key(component_id)
            {
                return Err(DevupError::with_details(
                    ErrorCode::DevupFigmaNodeNotFound,
                    "componentTsx needs a component master outside this snapshot. Re-collect the component master, or request tsx to inline the instance.",
                    false,
                    json!({"componentId":component_id,"instanceNodeId":id}),
                ));
            }
            pending.extend(view.child_ids());
        }
    }
    generate_component(snapshot, target.map_or(node_id, |node| node.id.as_str()), options)
}

fn component_definition_actions(
    payload: &CollectedPayload,
    tsx: &str,
    result: &Map<String, Value>,
) -> Vec<Value> {
    payload.snapshot.nodes.values()
        .filter(|node| node.node_type == "COMPONENT_SET")
        .filter_map(|node| {
            let name = node.typed_view().name()?;
            let name = devup_mcp_devup_ui::codegen::normalize_component_name(name);
            tsx.contains(&format!("from \"@/components/{name}\"" )).then(|| json!({
                "name": name, "nodeId": node.id,
                "nextAction": {"tool":"devup_figma_export", "arguments":{
                    "artifactId":result["cache"]["artifactId"],
                    "frameIds":[node.id], "outputs":["componentTsx"]
                }}
            }))
        })
        .collect()
}
