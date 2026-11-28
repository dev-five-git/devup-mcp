const STATE_WORDS: &[&str] = &[
    "default", "hover", "active", "focus", "selected", "disabled", "error", "loading", "empty",
];

/// True when `node` is `ancestor` or lies inside it in the snapshot tree.
fn within(payload: Option<&CollectedPayload>, node: &str, ancestor: &str) -> bool {
    if node == ancestor {
        return true;
    }
    let Some(payload) = payload else {
        return false;
    };
    let mut current = node.to_owned();
    let mut seen = BTreeSet::new();
    while seen.insert(current.clone()) {
        let Some(parent) = payload
            .snapshot
            .nodes
            .get(&current)
            .and_then(|n| n.typed_view().string("parentId").map(str::to_owned))
        else {
            return false;
        };
        if parent == ancestor {
            return true;
        }
        current = parent;
    }
    false
}

/// The states a node's component set declares through its variants.
fn variant_states(payload: &CollectedPayload, node_id: &str) -> BTreeSet<String> {
    let nodes = &payload.snapshot.nodes;
    let mut states = BTreeSet::new();
    let Some(node) = nodes.get(node_id) else {
        return states;
    };
    let view = node.typed_view();
    let master = if node.node_type == "INSTANCE" {
        view.string("mainComponentId").and_then(|id| nodes.get(id))
    } else {
        Some(node)
    };
    let set = view
        .string("mainComponentSetId")
        .and_then(|id| nodes.get(id))
        .or_else(|| {
            let parent = master?.typed_view().string("parentId")?;
            nodes.get(parent)
        })
        .filter(|set| set.node_type == "COMPONENT_SET");
    let Some(set) = set else {
        return states;
    };
    for child in set.typed_view().child_ids().filter_map(|id| nodes.get(id)) {
        let Some(variants) = child
            .typed_view()
            .value("variantProperties")
            .and_then(Value::as_object)
        else {
            continue;
        };
        for (key, value) in variants {
            let Some(value) = value.as_str() else {
                continue;
            };
            let key = key.to_ascii_lowercase();
            let state = normalize_state(value);
            if key.contains("state")
                || key.contains("status")
                || key.contains("interaction")
                || STATE_WORDS.contains(&state.as_str())
            {
                states.insert(state);
            }
        }
    }
    states
}

/// Every place the design data is incomplete: caller-declared gaps, failed
/// screens, error diagnostics, failed asset exports, unreadable fields and
/// children referenced but never collected under the nodes being verified.
fn collect_gaps(
    ctx: &MatrixContext<'_>,
    input: &MatrixInput<'_>,
    focus: &BTreeSet<String>,
) -> Vec<Value> {
    let mut gaps = input
        .design_gaps
        .iter()
        .map(|g| json!({"nodeId":g.node_id,"property":g.property,"kind":g.kind,"cause":g.cause,"source":"caller"}))
        .collect::<Vec<_>>();
    if let Some(reason) = &ctx.artifact_unavailable {
        gaps.push(json!({"nodeId":"*","property":null,"kind":"read-failed","cause":reason,"source":"artifact"}));
    }
    let Some(payload) = ctx.payload else {
        return gaps;
    };
    for failure in &payload.failures {
        gaps.push(json!({"nodeId":failure.node_id,"property":null,"kind":"read-failed",
            "cause":format!("{:?}: {}", failure.error_code, failure.message),"retryable":failure.retryable,"source":"export"}));
    }
    for diagnostic in &payload.snapshot.diagnostics {
        let severe = diagnostic.severity == Some(DiagnosticSeverity::Error)
            || matches!(
                diagnostic.fidelity_impact,
                Some(FidelityImpact::Failed | FidelityImpact::Lossy)
            );
        if let (true, Some(node)) = (severe, &diagnostic.node_id) {
            let kind = if diagnostic.property.is_some() {
                "missing-property"
            } else {
                "read-failed"
            };
            gaps.push(json!({"nodeId":node,"property":diagnostic.property,"kind":kind,
                "cause":format!("{}: {}", diagnostic.code, diagnostic.message),"source":"snapshot-diagnostic"}));
        }
    }
    for asset in payload
        .assets
        .iter()
        .filter(|a| a.status == devup_mcp_figma::AssetStatus::Failed)
    {
        gaps.push(json!({"nodeId":asset.node_id,"property":asset.field,"kind":"export-failed",
            "cause":format!("{}{}", asset.error_code.as_deref().unwrap_or("asset export failed"),
                asset.error_detail.as_deref().map(|d| format!(": {d}")).unwrap_or_default()),"source":"asset-export"}));
    }
    // Unreadable fields and missing children only matter under verified nodes.
    let mut pending = focus.iter().cloned().collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id.clone()) || seen.len() > 5_000 {
            continue;
        }
        let Some(node) = payload.snapshot.nodes.get(&id) else {
            if !focus.contains(&id) {
                continue;
            }
            gaps.push(json!({"nodeId":id,"property":null,"kind":"missing-node","cause":"The node is not in the cached snapshot (not collected, paginated out or failed to read).","source":"snapshot"}));
            continue;
        };
        for (field, reason) in &node.field_errors {
            gaps.push(
                json!({"nodeId":id,"property":field,"kind":"missing-property","cause":reason,
                "affectedProperties":field_properties(field),"source":"snapshot"}),
            );
        }
        for child in node.typed_view().child_ids() {
            if payload.snapshot.nodes.contains_key(child) {
                pending.push(child.to_owned());
            } else {
                gaps.push(json!({"nodeId":child,"property":null,"kind":"missing-node","referencedBy":id,
                    "cause":"Listed as a child but absent from the cached snapshot.","source":"snapshot"}));
            }
        }
    }
    gaps
}

/// Gaps that make a node's comparison incomplete: on the node or inside it.
fn gaps_for(payload: Option<&CollectedPayload>, gaps: &[Value], node: &str) -> Vec<Value> {
    gaps.iter()
        .filter(|g| {
            let id = text(&g["nodeId"]);
            id == "*"
                || within(payload, id, node)
                || g["referencedBy"]
                    .as_str()
                    .is_some_and(|p| within(payload, p, node))
        })
        .map(|g| json!({"nodeId":g["nodeId"],"property":g["property"],"kind":g["kind"]}))
        .collect()
}

/// Evaluate one state check. Async only for the optional pixel comparison,
/// which goes through the existing devup_visual_compare path unchanged.
async fn evaluate_state(ctx: &MatrixContext<'_>, check: &StateCheck, gaps: &[Value]) -> Value {
    let state = normalize_state(&check.state);
    let mut design = BTreeMap::new();
    let mut design_gaps = vec![];
    let node = present(&check.figma_node_id);
    if let (Some(node), Some(payload)) = (node, ctx.payload) {
        design = design_from_snapshot(payload, node);
    }
    if let Some(node) = node {
        design_gaps = gaps_for(ctx.payload, gaps, node);
    }
    if let Some(explicit) = &check.design {
        design.extend(snapshot_props(explicit));
    }
    let implementation = check
        .implementation
        .as_ref()
        .map(snapshot_props)
        .unwrap_or_default();
    let tolerance = check.tolerance_px.unwrap_or(0.5).clamp(0.0, 8.0);
    let comparisons = compare_props(&design, &implementation, tolerance);
    let visual = match (&check.visual, ctx.policy) {
        (None, _) => None,
        (Some(_), None) => Some(
            json!({"verdict":"error","message":"Pixel comparison needs the server output policy; call through devup_feature_trace on a running server."}),
        ),
        (Some(visual), Some(policy)) => Some(
            match super::visual_compare::compare(visual.clone(), policy, ctx.store).await {
                Ok(result) => {
                    json!({"verdict":result["verdict"],"changedBounds":result["visual"]["changedBounds"],
                "changedRatio":result["visual"]["changedRatio"],"environment":result["environment"]["status"],
                "diffPath":result["diffPath"],"nextAction":result["nextAction"],"tool":"devup_visual_compare"})
                }
                Err(error) => {
                    json!({"verdict":"error","message":error.to_string(),"tool":"devup_visual_compare"})
                }
            },
        ),
    };
    let mismatches = comparisons
        .iter()
        .filter(|c| c["status"] == "mismatch")
        .count();
    let not_measured = comparisons
        .iter()
        .filter(|c| c["status"] == "not-measured")
        .map(|c| c["property"].clone())
        .collect::<Vec<_>>();
    let visual_verdict = visual.as_ref().map(|v| text(&v["verdict"]).to_owned());
    let has_design = !design.is_empty() || check.visual.is_some();
    let has_capture = !implementation.is_empty() || check.visual.is_some();
    let observed = check.implementation_source.observed();
    let static_only = !implementation.is_empty() && !observed;
    let (verdict, reason) = if mismatches > 0 || visual_verdict.as_deref() == Some("fail") {
        (
            "failed",
            "The implementation differs from the design; fix it and verify again.",
        )
    } else if !has_design {
        (
            "unverified",
            "No design for this state: no Figma node values, design values or visual reference were supplied.",
        )
    } else if !has_capture {
        (
            "unverified",
            "No capture of this state: no implementation values or screenshot were supplied.",
        )
    } else if !design_gaps.is_empty() {
        (
            "unverified",
            "The Figma data for this state is incomplete; a match on partial data is not a design match.",
        )
    } else if !not_measured.is_empty() {
        (
            "unverified",
            "Some design properties were not measured on the implementation.",
        )
    } else if visual_verdict.as_deref().is_some_and(|v| v != "pass") {
        (
            "unverified",
            "The pixel comparison was inconclusive or could not run.",
        )
    } else if static_only {
        (
            "unverified",
            "Values were read from source only; the rendered state needs browser verification.",
        )
    } else {
        (
            "verified",
            "Every design property matched an observed implementation value.",
        )
    };
    let capture_needed = verdict == "failed" && check.visual.is_none();
    json!({"target":check.target,"routePath":check.route_path,"state":state,"declaredState":check.state,
        "figmaNodeId":check.figma_node_id,"verdict":verdict,"reason":reason,
        "implementationSource":check.implementation_source.as_str(),
        "browserVerificationRequired":verdict != "failed" && static_only,
        "comparisons":comparisons,"mismatchCount":mismatches,"notMeasured":not_measured,
        "visual":visual,"designDataGaps":design_gaps,
        "capture":{"needed":capture_needed,"diffPath":visual.as_ref().map(|v| v["diffPath"].clone()),
            "rule":"Capture only for a visual comparison or to analyse a failure."},
        "attempts":check.attempts,"blocker":check.blocker})
}

/// Group states by component/screen and list expected states nobody checked.
async fn state_groups(
    ctx: &MatrixContext<'_>,
    input: &MatrixInput<'_>,
    gaps: &[Value],
) -> Vec<Value> {
    let mut groups = BTreeMap::<(String, Option<String>), (Vec<Value>, BTreeSet<String>)>::new();
    for check in input.state_checks {
        let row = evaluate_state(ctx, check, gaps).await;
        let key = (
            normalize_path(&check.target),
            present(&check.route_path).map(str::to_owned),
        );
        let entry = groups.entry(key).or_default();
        entry
            .1
            .extend(check.expected_states.iter().map(|s| normalize_state(s)));
        if let (Some(payload), Some(node)) = (ctx.payload, present(&check.figma_node_id)) {
            entry.1.extend(variant_states(payload, node));
        }
        entry.0.push(row);
    }
    groups
        .into_iter()
        .map(|((target, route), (mut states, expected))| {
            let checked = states.iter().map(|s| text(&s["state"]).to_owned()).collect::<BTreeSet<_>>();
            for missing in expected.difference(&checked) {
                states.push(json!({"target":target,"routePath":route,"state":missing,"verdict":"unverified",
                    "reason":"Expected state with no design comparison or capture supplied; it is not a pass.",
                    "missing":true,"browserVerificationRequired":false,"comparisons":[],"capture":{"needed":false}}));
            }
            let count = |v: &str| states.iter().filter(|s| s["verdict"] == v).count();
            json!({"target":target,"routePath":route,"expectedStates":expected,
                "counts":{"verified":count("verified"),"failed":count("failed"),"unverified":count("unverified")},
                "states":states})
        })
        .collect()
}
