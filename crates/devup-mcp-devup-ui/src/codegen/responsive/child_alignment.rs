struct MergeContext<'a> {
    snapshot: &'a Snapshot,
    notes: Vec<Unrepresented>,
    diagnostics: Vec<devup_mcp_figma::Diagnostic>,
    slot_widths: BySlot<u32>,
}

fn role_kind(tree: &Tree) -> &str {
    if tree.is_component {
        "component"
    } else if LAYOUT_ELEMENTS.contains(&tree.component.as_str()) {
        "layout"
    } else {
        &tree.component
    }
}

fn role(tree: &Tree) -> String {
    let kind = role_kind(tree);
    format!(
        "{kind}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}",
        tree.content
            .as_deref()
            .map(|text| text.replace("<br />", "")),
        tree.props
            .get("src")
            .or_else(|| tree.props.get("maskImage")),
        tree.props.get("srcSet"),
        tree.props.get("sizes"),
        tree.props.get("width"),
        tree.props.get("height")
    )
}

fn responsive_signature(tree: &Tree) -> String {
    format!(
        "{}:{}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:[{}]",
        role_kind(tree),
        tree.content.is_some(),
        tree.props.get("src"),
        tree.props.get("srcSet"),
        tree.props.get("sizes"),
        tree.props.get("width"),
        tree.props.get("height"),
        tree.props.get("maskImage"),
        tree.children
            .iter()
            .map(responsive_signature)
            .collect::<Vec<_>>()
            .join("|")
    )
}

fn component_identity<'a>(snapshot: &'a Snapshot, tree: &Tree, set: bool) -> Option<&'a str> {
    let node = snapshot.nodes.get(&tree.node_id)?;
    let view = node.typed_view();
    if set {
        view.string("mainComponentSetId").or_else(|| {
            let master = view
                .string("mainComponentId")
                .or_else(|| view.string("componentId"))?;
            let parent = snapshot
                .nodes
                .get(master)?
                .typed_view()
                .string("parentId")?;
            (snapshot.nodes.get(parent)?.node_type == "COMPONENT_SET").then_some(parent)
        })
    } else {
        view.string("mainComponentId")
            .or_else(|| view.string("componentId"))
    }
}

fn matches_at(snapshot: &Snapshot, left: &Tree, right: &Tree, pass: usize) -> bool {
    if ["src", "srcSet", "sizes", "width", "height", "maskImage"]
        .iter()
        .any(|key| left.props.get(*key) != right.props.get(*key))
    {
        return false;
    }
    if pass == 2 && role(left) != role(right) {
        return false;
    }
    match pass {
        0 | 1 => component_identity(snapshot, left, pass == 1)
            .is_some_and(|id| component_identity(snapshot, right, pass == 1) == Some(id)),
        2 => left.node_name == right.node_name,
        _ => {
            (left.node_name == right.node_name || role(left) == role(right))
                && responsive_signature(left) == responsive_signature(right)
        }
    }
}

type ChildMaps<'a> = BySlot<Vec<(String, Vec<&'a Tree>)>>;

fn align_children<'a>(by_slot: &'a BySlot<Tree>, context: &mut MergeContext<'_>) -> ChildMaps<'a> {
    let mut groups: Vec<BySlot<&Tree>> = Vec::new();
    let mut maps: ChildMaps<'a> =
        std::array::from_fn(|slot| by_slot[slot].as_ref().map(|_| Vec::new()));
    for slot in 0..SLOTS {
        let Some(parent) = &by_slot[slot] else {
            continue;
        };
        let mut assigned = vec![None; parent.children.len()];
        for pass in 0..4 {
            let candidates = parent
                .children
                .iter()
                .enumerate()
                .map(|(index, child)| {
                    if assigned[index].is_some() {
                        return Vec::new();
                    }
                    groups
                        .iter()
                        .enumerate()
                        .filter(|(_, group)| {
                            group[slot].is_none()
                                && group
                                    .iter()
                                    .flatten()
                                    .any(|other| matches_at(context.snapshot, child, other, pass))
                        })
                        .map(|(index, _)| index)
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            for (index, choices) in candidates.iter().enumerate() {
                if let [group] = choices.as_slice()
                    && candidates
                        .iter()
                        .filter(|choices| choices.contains(group))
                        .count()
                        == 1
                {
                    assigned[index] = Some(*group);
                    groups[*group][slot] = Some(&parent.children[index]);
                }
            }
            if pass == 2 {
                for (index, choices) in candidates.iter().enumerate() {
                    if assigned[index].is_some() || choices.is_empty() {
                        continue;
                    }
                    let peers = candidates
                        .iter()
                        .enumerate()
                        .filter(|(other, candidate)| {
                            assigned[*other].is_none() && *candidate == choices
                        })
                        .map(|(index, _)| index)
                        .collect::<Vec<_>>();
                    let available = choices
                        .iter()
                        .filter(|group| groups[**group][slot].is_none())
                        .copied()
                        .collect::<Vec<_>>();
                    if peers.len() == available.len() {
                        for (child, group) in peers.into_iter().zip(available) {
                            assigned[child] = Some(group);
                            groups[group][slot] = Some(&parent.children[child]);
                        }
                    }
                }
            }
        }
        for (index, child) in parent.children.iter().enumerate() {
            let group = match assigned[index] {
                Some(group) => group,
                None => {
                    let candidates = groups
                        .iter()
                        .filter(|group| {
                            group[slot].is_none()
                                && group.iter().flatten().any(|other| {
                                    (0..4).any(|pass| {
                                        matches_at(context.snapshot, child, other, pass)
                                    })
                                })
                        })
                        .collect::<Vec<_>>();
                    if !candidates.is_empty() {
                        let ids = candidates.iter().flat_map(|group| group.iter().enumerate().filter_map(|(slot, tree)| tree.map(|tree| json!({"slot":slot,"nodeId":tree.node_id,"width":context.slot_widths[slot]})))).chain(std::iter::once(json!({"slot":slot,"nodeId":child.node_id,"width":context.slot_widths[slot]}))).collect::<Vec<_>>();
                        context.diagnostics.push(devup_mcp_figma::Diagnostic {
                            code: "DEVUP_CODEGEN_RESPONSIVE_UNMATCHED".into(),
                            message: "Responsive child identity is ambiguous; separate width-specific nodes were retained.".into(),
                            node_id: Some(child.node_id.clone()),
                            details: Some(json!({"nodesByWidth":ids,"reason":"multiple semantic counterparts","nextAction":"Give ambiguous siblings distinct names or collect their mainComponentId/componentSet identity.","output":"responsiveTsx"})),
                            severity: Some(devup_mcp_figma::DiagnosticSeverity::Warning),
                            fidelity_impact: Some(devup_mcp_figma::FidelityImpact::Approximated),
                            ..devup_mcp_figma::Diagnostic::default()
                        });
                    }
                    let group = groups.len();
                    let mut members = [None; SLOTS];
                    members[slot] = Some(child);
                    groups.push(members);
                    group
                }
            };
            if let Some(map) = &mut maps[slot] {
                map.push((group.to_string(), vec![child]));
            }
        }
    }
    maps
}
