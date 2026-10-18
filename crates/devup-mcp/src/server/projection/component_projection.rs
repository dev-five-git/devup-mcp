/// Marks the page root, its top-level children (the sections), their
/// children and every asset with `data-figma-node-id`, so the runtime audit captures a section
/// or an asset by the node it came from. Only for exports into a project
/// (`projectRoot`); `.devup/screen-plan.config.json` `figmaNodeIds: false`
/// turns it off.
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
    if !options.inline_instances {
        let mut pending = vec![node_id];
        let mut visited = std::collections::BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            let Some(node) = payload.snapshot.nodes.get(id) else {
                continue;
            };
            let view = node.typed_view();
            if view.bool("visible") == Some(false) {
                continue;
            }
            if node.node_type == "INSTANCE" {
                let component_id = view
                    .string("mainComponentId")
                    .or_else(|| view.string("componentId"))
                    .or_else(|| view.string("mainComponent"))
                    .or_else(|| {
                        view.value("mainComponent")
                            .and_then(|value| value.get("id"))
                            .and_then(Value::as_str)
                    });
                if let Some(component_id) = component_id
                    && !payload.snapshot.nodes.contains_key(component_id)
                {
                    return Err(DevupError::with_details(
                        ErrorCode::DevupFigmaNodeNotFound,
                        "componentTsx needs a component master outside this snapshot. Re-collect the component master, or request tsx to inline the instance.",
                        false,
                        json!({"componentId":component_id,"instanceNodeId":id}),
                    ));
                }
            }
            pending.extend(view.child_ids());
        }
    }
    generate_component(&payload.snapshot, node_id, options)
}
