/// Keep the semantic correspondence chosen by align_children. CSS order is
/// derived only for complete, uniquely matched native flex sibling sets.
fn captured_child_orders(
    parents: &BySlot<Tree>,
    maps: &ChildMaps<'_>,
    context: &mut MergeContext<'_>,
) -> Option<BySlot<BTreeMap<String, usize>>> {
    let drawn = maps.iter().flatten().collect::<Vec<_>>();
    if drawn.len() < 2
        || !drawn
            .iter()
            .any(|left| drawn.iter().any(|right| shared_order_differs(left, right)))
    {
        return None;
    }
    for (slot, map) in maps
        .iter()
        .enumerate()
        .filter_map(|(slot, map)| map.as_ref().map(|map| (slot, map)))
    {
        let parent = parents[slot].as_ref()?;
        let node = context.snapshot.nodes.get(&parent.node_id)?;
        if parent.is_component
            || !matches!(
                node.typed_view().string("layoutMode"),
                Some("HORIZONTAL" | "VERTICAL")
            )
            || node.field_errors.contains_key("childrenIds")
            || node.field_errors.contains_key("layoutMode")
        {
            return None;
        }
        let source_ids = node.fields.get("childrenIds")?.as_array()?;
        if source_ids.len() != map.len()
            || source_ids.iter().zip(map).any(|(id, (_, children))| {
                children.len() != 1
                    || id.as_str() != Some(children[0].node_id.as_str())
                    || children[0]
                        .props
                        .get("pos")
                        .is_some_and(|v| v == "absolute")
                    || children[0].props.contains_key("order")
            })
        {
            return None;
        }
        for (key, children) in map {
            for other_map in &drawn {
                if let Some((_, peers)) = other_map.iter().find(|(other, _)| other == key) {
                    let other = *peers.first()?;
                    if !(0..4).any(|pass| {
                        matches_at(context.snapshot, children[0], other, pass)
                            && map
                                .iter()
                                .filter(|(_, peers)| {
                                    matches_at(context.snapshot, peers[0], other, pass)
                                })
                                .count()
                                == 1
                            && other_map
                                .iter()
                                .filter(|(_, peers)| {
                                    matches_at(context.snapshot, children[0], peers[0], pass)
                                })
                                .count()
                                == 1
                    }) {
                        return None;
                    }
                } else if other_map.iter().any(|(_, peers)| {
                    (0..4).any(|pass| matches_at(context.snapshot, children[0], peers[0], pass))
                }) {
                    // A width-specific layer is safe only when no possible
                    // counterpart was left unmatched at the other width.
                    return None;
                }
            }
        }
    }
    context.diagnostics.push(devup_mcp_figma::Diagnostic {
        code: "DEVUP_CODEGEN_RESPONSIVE_READING_ORDER_REVIEW".into(),
        message: "Captured flex sibling order differs by viewport. CSS order preserves each visual order; DOM reading and keyboard order retain the canonical tree and require semantic review for interactive content.".into(),
        node_id: parents.iter().flatten().next().map(|p| p.node_id.clone()),
        details: Some(json!({"sourceField":"childrenIds","nodes":parents.iter().flatten().map(|p| &p.node_id).collect::<Vec<_>>(),"output":"responsiveTsx","duplicatesDOM":false})),
        severity: Some(devup_mcp_figma::DiagnosticSeverity::Warning),
        ..devup_mcp_figma::Diagnostic::default()
    });
    Some(std::array::from_fn(|slot| {
        maps[slot].as_ref().map(|map| {
            map.iter()
                .enumerate()
                .map(|(index, (key, _))| (key.clone(), index))
                .collect()
        })
    }))
}

fn captured_order_provenance(snapshot: &Snapshot, tree: &Tree) -> Vec<Value> {
    tree.slots.iter().enumerate().filter_map(|(slot, source)| {
        let (id, _) = source.as_ref()?;
        let parent_id = snapshot.nodes.get(id)?.typed_view().string("parentId")?;
        let ids = snapshot.nodes.get(parent_id)?.fields.get("childrenIds")?.as_array()?;
        let index = ids.iter().position(|child| child.as_str() == Some(id))?;
        Some(json!({"slot":slot,"sourceNodeId":parent_id,"sourceField":"childrenIds","childNodeId":id,"sourceIndex":index,"calculation":"CSS order equals captured direct flex sibling index; DOM reading order is unchanged"}))
    }).collect()
}

/// Width-specific siblings do not by themselves create an order conflict.
/// Compare only jointly captured peers; their positions remain exact indices
/// in each complete source list, including any width-specific spacer.
fn shared_order_differs(left: &[(String, Vec<&Tree>)], right: &[(String, Vec<&Tree>)]) -> bool {
    let common_left = left
        .iter()
        .filter(|(key, _)| right.iter().any(|(other, _)| other == key))
        .map(|(key, _)| key)
        .collect::<Vec<_>>();
    let common_right = right
        .iter()
        .filter(|(key, _)| left.iter().any(|(other, _)| other == key))
        .map(|(key, _)| key)
        .collect::<Vec<_>>();
    common_left.len() > 1 && common_left != common_right
}
