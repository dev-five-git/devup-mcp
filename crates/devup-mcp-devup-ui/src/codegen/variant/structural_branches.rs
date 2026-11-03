// Preserve captured variant topology instead of substituting a default tree.

fn viewport_slot(value: &str) -> Option<usize> {
    match value.to_ascii_lowercase().as_str() {
        "mobile" => Some(0),
        "tablet" => Some(2),
        "desktop" | "pc" => Some(4),
        _ => None,
    }
}

fn branch_display(value: &str, options: &[String], display: &str) -> Option<String> {
    let selected = viewport_slot(value)?;
    let mut slots = vec![None::<String>; 5];
    slots[0] = Some("none".into());
    for option in options {
        let slot = viewport_slot(option)?;
        slots[slot] = Some(if slot == selected { display } else { "none" }.into());
    }
    while slots.last().is_some_and(Option::is_none) {
        slots.pop();
    }
    Some(serde_json::to_string(&slots).expect("literal display slots"))
}

fn natural_branch_display(component: &str) -> &str {
    match component {
        "Flex" | "VStack" | "Center" => "flex",
        "Grid" => "grid",
        "Image" => "inline",
        _ => "block",
    }
}

/// The exact root gate the branch renderer writes, replayed by property audit.
pub(super) fn display_gate_derivation(
    snapshot: &Snapshot,
    node: &RawNode,
    component: &str,
    props: &[super::component::Prop],
) -> Option<Value> {
    if node.node_type != "COMPONENT" {
        return None;
    }
    let view = node.typed_view();
    let parent = snapshot.nodes.get(view.string("parentId")?)?;
    if parent.node_type != "COMPONENT_SET" {
        return None;
    }
    let viewport = definitions(parent)
        .into_iter()
        .find(|d| d.name.eq_ignore_ascii_case("viewport"))?;
    if viewport.options.len() < 2 {
        return None;
    }
    let selected = view
        .value("variantProperties")?
        .get(&viewport.source)?
        .as_str()?;
    let display = props
        .iter()
        .find_map(|(name, value)| {
            (name == "display").then_some(match value {
                PropValue::String(value) => value.as_str(),
            })
        })
        .unwrap_or_else(|| natural_branch_display(component));
    let value = branch_display(selected, &viewport.options, display)?;
    Some(
        serde_json::json!({"stage":"variant-branch","generatedProperty":super::component::render_static_attribute("display",&value),
        "sourceFields":["variantProperties","parentId"],"originalValue":{"variantProperties":view.value("variantProperties"),"parentComponentSet":parent.id,"viewportOptions":viewport.options},
        "calculation":"Captured viewport branches are separate roots with CSS display gates at Mobile/Tablet/Desktop slots. The visible branch restores its captured/natural display; no layout wrapper or geometry is added."}),
    )
}

fn branch_issue(code: &str, node: &str, message: &str, details: Value) -> Diagnostic {
    Diagnostic {
        code: code.into(),
        message: message.into(),
        node_id: Some(node.into()),
        severity: Some(DiagnosticSeverity::Warning),
        fidelity_impact: Some(FidelityImpact::Lossy),
        details: Some(details),
        ..Default::default()
    }
}

fn rename_text_binding(tree: &mut Tree, from: &str, to: &str) {
    if tree.content.as_deref() == Some(&format!("{{{from}}}")) {
        tree.content = Some(format!("{{{to}}}"));
    }
    for child in &mut tree.children {
        rename_text_binding(child, from, to);
    }
}

fn effect_branch_issues(base: &Tree, other: &Tree, node_id: &str, out: &mut Vec<Diagnostic>) {
    if structure_signature(base) != structure_signature(other) {
        out.push(branch_issue("DEVUP_CODEGEN_EFFECT_TOPOLOGY_UNSUPPORTED",node_id,
            "An interaction effect changes captured topology. Its baseline subtree is retained; the effect topology requires an explicit interaction owner.",
            serde_json::json!({"classification":"effect-topology","baselineNodeId":base.node_id,"effectNodeId":other.node_id,
                "nextAction":"Implement the captured effect subtree using an explicit shared hover/focus/pressed owner, or report this effect as unresolved. Do not substitute default geometry and claim exactness."})));
        return;
    }
    for (left, right) in base.children.iter().zip(&other.children) {
        if left.props != right.props || left.content != right.content {
            out.push(branch_issue("DEVUP_CODEGEN_EFFECT_DESCENDANT_UNSUPPORTED",&right.node_id,
                "A component interaction effect changes a descendant. Root state styles do not prove that descendant state behavior is implemented.",
                serde_json::json!({"classification":"effect-descendant","baselineNodeId":left.node_id,"effectNodeId":right.node_id,
                    "nextAction":"Bind the descendant's captured styles to the parent interaction owner and verify the state at runtime."})));
        }
        effect_branch_issues(left, right, node_id, out);
    }
}

fn preserve_structural_variants(
    snapshot: &Snapshot,
    set: &RawNode,
    records: &[Record],
    definitions: &[Definition],
) -> Result<Option<CodegenOutput>, DevupError> {
    let Some(first) = records.first() else {
        return Ok(None);
    };
    let topology = |tree: &Tree| {
        (
            tree.component.clone(),
            tree.content.is_some(),
            tree.children
                .iter()
                .map(structure_signature)
                .collect::<Vec<_>>(),
        )
    };
    if records
        .iter()
        .all(|record| topology(&record.tree) == topology(&first.tree))
    {
        return Ok(None);
    }
    let effect = definitions.iter().find(|d| {
        d.name.eq_ignore_ascii_case("effect")
            && records
                .iter()
                .all(|record| record.tree.component != "Image")
            && d.options.iter().all(|value| {
                value.eq_ignore_ascii_case(&d.default) || effect_selector(value).is_some()
            })
    });
    let viewport = definitions
        .iter()
        .find(|d| d.name.eq_ignore_ascii_case("viewport"));
    let mut public_dimensions = definitions
        .iter()
        .filter(|d| {
            !d.name.eq_ignore_ascii_case("viewport")
                && effect.is_none_or(|effect| d.name != effect.name)
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut groups = BTreeMap::<BTreeMap<String, String>, Vec<Record>>::new();
    for record in records {
        let group = record
            .values
            .iter()
            .filter(|(name, _)| effect.is_none_or(|e| *name != &e.source))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        groups.entry(group).or_default().push(record.clone());
    }
    let mut imports = BTreeSet::new();
    let mut text_properties = BTreeMap::<String, String>::new();
    let mut diagnostics = Vec::new();
    let mut branches = Vec::new();
    for (fixed, group) in groups {
        let baseline = effect
            .and_then(|e| {
                group
                    .iter()
                    .find(|record| record.values.get(&e.source) == Some(&e.default))
            })
            .unwrap_or(&group[0]);
        if let Some(effect) = effect {
            for record in &group {
                if record.node_id != baseline.node_id {
                    effect_branch_issues(
                        &baseline.tree,
                        &record.tree,
                        &record.node_id,
                        &mut diagnostics,
                    );
                }
            }
            if !group
                .iter()
                .any(|record| record.values.get(&effect.source) == Some(&effect.default))
            {
                diagnostics.push(branch_issue("DEVUP_CODEGEN_EFFECT_BASELINE_MISSING",&baseline.node_id,
                    "This captured variant combination has no declared default interaction state. Its first capture is retained explicitly.",
                    serde_json::json!({"classification":"missing-effect-baseline","fixedVariants":fixed,"defaultEffect":effect.default})));
            }
        }
        let scoped_definitions = definitions
            .iter()
            .map(|definition| {
                let mut scoped = definition.clone();
                if let Some(value) = fixed.get(&definition.source) {
                    scoped.default = value.clone();
                    scoped.options = vec![value.clone()];
                }
                scoped
            })
            .collect::<Vec<_>>();
        let scoped_dimensions = scoped_definitions
            .iter()
            .filter(|d| {
                !d.name.eq_ignore_ascii_case("viewport") && effect.is_none_or(|e| d.name != e.name)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut root = baseline.tree.clone();
        root.props = merged_props(
            &group,
            &scoped_definitions,
            &scoped_dimensions,
            None,
            effect.map_or("default", |e| e.default.as_str()),
            None,
            &baseline.tree.props,
        );
        merge_child_props(
            &mut root,
            &group,
            &scoped_definitions,
            &scoped_dimensions,
            None,
            effect.map_or("default", |e| e.default.as_str()),
            &[],
        );
        bind_referenced_properties(&mut root, &mut public_dimensions);
        let branch_text = bind_text_properties(snapshot, set, &mut root, &public_dimensions);
        for (name, default) in branch_text {
            let mut selected = name.clone();
            let mut suffix = 2;
            while text_properties
                .get(&selected)
                .is_some_and(|value| value != &default)
            {
                selected = format!("{name}Variant{suffix}");
                suffix += 1;
            }
            if selected != name {
                rename_text_binding(&mut root, &name, &selected);
            }
            text_properties.insert(selected, default);
        }
        let mut selectors = BTreeMap::new();
        let mut transitioned = BTreeSet::new();
        if let Some(effect) = effect {
            for value in &effect.options {
                if value == &effect.default {
                    continue;
                }
                let changed = changed_effect_props(
                    &group,
                    &scoped_definitions,
                    &scoped_dimensions,
                    None,
                    &effect.default,
                    value,
                );
                if !changed.is_empty()
                    && let Some(selector) = effect_selector(value)
                {
                    transitioned.extend(changed.keys().cloned());
                    selectors.insert(selector.into(), changed);
                }
            }
        }
        if let Some(viewport) = viewport.filter(|d| d.options.len() > 1) {
            let selected = fixed
                .get(&viewport.source)
                .map(String::as_str)
                .unwrap_or(&viewport.default);
            let captured = baseline
                .tree
                .props
                .get("display")
                .map(String::as_str)
                .unwrap_or_else(|| natural_branch_display(&root.component));
            if let Some(value) = branch_display(selected, &viewport.options, captured) {
                root.props.insert("display".into(), format!("={{{value}}}"));
            } else {
                root.props.insert("display".into(), "=\"none\"".into());
                diagnostics.push(branch_issue("DEVUP_CODEGEN_VARIANT_VIEWPORT_UNSUPPORTED",&baseline.node_id,
                    "A captured viewport label has no established responsive slot. The retained branch stays hidden rather than guessing its breakpoint.",
                    serde_json::json!({"classification":"unknown-viewport","viewport":selected,"options":viewport.options,
                        "nextAction":"Map the named viewport to a confirmed breakpoint before making this branch visible."})));
            }
        }
        let guards = public_dimensions
            .iter()
            .filter(|d| !d.boolean)
            .filter_map(|d| {
                fixed.get(&d.source).map(|value| {
                    format!("{} === {}", d.name, serde_json::to_string(value).unwrap())
                })
            })
            .collect::<Vec<_>>();
        root.drawn_when.extend(guards);
        collect_imports(&root, &mut imports);
        let transition = transition(snapshot, &baseline.node_id, &transitioned);
        branches.push(render_tree(&root, &selectors, transition.as_ref(), 3));
    }
    let body = format!("    <>\n{}\n    </>", branches.join("\n"));
    let name = legacy_component_name(set.typed_view().name().unwrap_or("Component"));
    let code = mark_node(
        &set.id,
        render_component_body(&name, &public_dimensions, &text_properties, &body),
    );
    diagnostics.push(Diagnostic {code:"DEVUP_CODEGEN_VARIANT_BRANCHES_PRESERVED".into(),
        message:"Different captured variant structures are emitted as explicit prop/viewport branches without adding layout wrappers.".into(),
        node_id:Some(set.id.clone()),severity:Some(DiagnosticSeverity::Info),fidelity_impact:Some(FidelityImpact::None),
        details:Some(serde_json::json!({"branches":branches.len(),"capturedRecords":records.len(),"viewportSlots":{"Mobile":0,"Tablet":2,"Desktop":4},"evidenceLimit":"Static captured branch preservation; browser interaction and geometry still require runtime verification."})),..Default::default()});
    Ok(Some(CodegenOutput {
        tsx: code,
        imports: imports.into_iter().collect(),
        used_tokens: BTreeSet::new(),
        diagnostics,
        source_map: crate::provenance::SourceMap::empty(),
        projection_trace: crate::provenance::ProjectionTrace::default(),
        fidelity_report: crate::provenance::FidelityReport::default(),
        width_policy: None,
    }))
}
