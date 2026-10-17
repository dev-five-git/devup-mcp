use super::*;
use devup_mcp_devup_ui::codegen::responsive::{
    breakpoint_family_is_possible, is_breakpoint_name, slot_of_width,
};
use devup_mcp_figma::ExploreCandidate;

pub fn discover(candidates: &[ExploreCandidate], artifact_id: &str) -> Vec<Value> {
    let mut buckets = BTreeMap::<(Option<String>, String), Vec<&ExploreCandidate>>::new();
    for candidate in candidates.iter().filter(|candidate| {
        candidate.node.is_screen_candidate()
            || (candidate.node.visible
                && candidate.node.node_type == "FRAME"
                && is_breakpoint_name(&candidate.node.name))
    }) {
        let family = devup_mcp_devup_ui::codegen::responsive::family_name(&candidate.node.name);
        buckets
            .entry((candidate.node.parent_id.clone(), family))
            .or_default()
            .push(candidate);
    }
    buckets.into_iter().filter_map(|((parent, name), mut frames)| {
        let names_widths = frames.iter().map(|frame| (frame.node.name.as_str(), frame.node.bounds.width)).collect::<Vec<_>>();
        let slots = frames.iter().filter_map(|frame| width_slot(frame.node.bounds.width)).collect::<std::collections::BTreeSet<_>>();
        // Repeated width bands are ambiguous (e.g. light/dark copies). Never
        // silently choose a winner or merge different screens on width alone.
        if !breakpoint_family_is_possible(&names_widths) || slots.len() != frames.len() { return None; }
        frames.sort_by(|left, right| left.node.bounds.width.total_cmp(&right.node.bounds.width));
        let ids = frames.iter().map(|frame| frame.node.node_id.clone()).collect::<Vec<_>>();
        Some(json!({"name":if name == "@breakpoint-labels" {"breakpoint variants"} else {&name},
            "parentId":parent,"state":"candidate","relationshipEvidence":"sibling names and distinct width bands",
            "structureVerified":false,"frameIds":ids,
            "breakpoints":frames.iter().map(|frame| json!({"nodeId":frame.node.node_id,"name":frame.node.name,
                "width":frame.node.bounds.width,"slot":width_slot(frame.node.bounds.width)})).collect::<Vec<_>>(),
            "nextAction":{"tool":"devup_figma_export","arguments":{"artifactId":artifact_id,"frameIds":ids,"outputs":["responsiveTsx"],"delivery":"resource"},
                "why":"Export these related widths together. The merge will report structural differences and unrepresented properties."},
            "completionRequirement":"Report implemented breakpoints and remaining breakpoints; generated code alone does not verify the application at those widths."}))
    }).collect()
}

/// Ambiguous correspondences remain review work even when the generated code
/// preserves every source value. Positional occurrence is not design intent.
pub fn review(snapshot: &devup_mcp_figma::Snapshot) -> Value {
    let found = devup_mcp_devup_ui::codegen::responsive::breakpoints(snapshot);
    let mut items = devup_mcp_devup_ui::codegen::responsive::divergences(snapshot,&found).into_iter()
        .map(|d|json!({"kind":"unmatched-structure","nodeId":d.node_id,"path":d.path,"reason":d.reason.as_str(),"state":"review-required"})).collect::<Vec<_>>();
    let mut pending = snapshot
        .roots
        .iter()
        .map(String::as_str)
        .collect::<std::collections::VecDeque<_>>();
    let mut visited = std::collections::BTreeSet::new();
    while let Some(id) = pending.pop_front() {
        if !visited.insert(id) || visited.len() > 100_000 {
            continue;
        }
        let Some(node) = snapshot.nodes.get(id) else {
            continue;
        };
        pending.extend(node.typed_view().child_ids());
        let mut grouped = BTreeMap::<(String, String), Vec<String>>::new();
        for id in node.typed_view().child_ids() {
            if let Some(child) = snapshot.nodes.get(id) {
                grouped
                    .entry((
                        child.typed_view().name().unwrap_or("").into(),
                        child.node_type.clone(),
                    ))
                    .or_default()
                    .push(id.into());
            }
        }
        for ((name, node_type), ids) in grouped.into_iter().filter(|(_, ids)| ids.len() > 1) {
            items.push(json!({"kind":"ambiguous-sibling-correspondence","parentId":node.id,"nodeIds":ids,"name":name,"nodeType":node_type,
                "state":"review-required","reason":"Repeated sibling names and types cannot prove correspondence; existing occurrence-order merging is a candidate only."}));
        }
    }
    json!({"items":items.iter().take(200).collect::<Vec<_>>(),"total":items.len(),"truncated":items.len()>200,
        "pairingEvidence":"explicit family names, node types and captured structure","implementationState":"unverified"})
}

pub fn width_evidence(snapshot: &devup_mcp_figma::Snapshot) -> Value {
    let frames = snapshot
        .roots
        .iter()
        .filter_map(|id| {
            let n = snapshot.nodes.get(id)?;
            let width = n.typed_view().number("width")?;
            Some(json!({"nodeId":id,"width":width,"slot":width_slot(width)}))
        })
        .collect::<Vec<_>>();
    let slots=(0..5).map(|slot|json!({"slot":slot,"state":if frames.iter().any(|f|f["slot"]==slot) {"drawn-reference"} else {"undefined"}})).collect::<Vec<_>>();
    json!({"frames":frames,"slots":slots,"intermediateWidths":"undefined unless explicitly drawn",
        "arraySemantics":"null inherits the preceding CSS value; it does not verify an intermediate design or choose an interpolation rule.",
        "fluidRecommendations":["Review equal columns with 1fr or minmax(0, 1fr)","Review flex wrapping and min-width constraints","Measure padding and card sizes at intermediate viewports"],
        "evidenceLimit":"Recommendations are review options, not inferred designer intent or verified intermediate behavior."})
}

pub fn from_roots(payload: &CollectedPayload, artifact_id: &str) -> Vec<Value> {
    let candidates = payload
        .snapshot
        .roots
        .iter()
        .filter_map(|id| {
            let node =
                devup_mcp_figma::ExploreNode::try_from(payload.snapshot.nodes.get(id)?).ok()?;
            node.parent_id.as_ref()?;
            Some(devup_mcp_figma::ExploreCandidate {
                canonical_url: payload.target.link(Some(id)),
                node,
                score: 0,
                selection_reasons: vec!["captured-root".into()],
            })
        })
        .collect::<Vec<_>>();
    let mut groups = discover(&candidates, artifact_id);
    for group in &mut groups {
        group["nextAction"]["arguments"]
            .as_object_mut()
            .unwrap()
            .remove("frameIds");
    }
    groups
}

fn width_slot(width: f64) -> Option<usize> {
    if !width.is_finite() || width <= 0.0 || width > f64::from(u32::MAX) {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "validated positive frame pixel width"
    )]
    Some(slot_of_width(width as u32))
}

pub fn completion(result: &mut Map<String, Value>, selected: &[String], all_screens: bool) {
    let Some(groups) = result.get("responsiveGroups").and_then(Value::as_array) else {
        return;
    };
    if groups.is_empty() {
        return;
    }
    let merged = result.get("responsiveTsx").is_some();
    let exported_ids = result
        .get("frames")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|frame| {
            ["tsx", "componentTsx"]
                .iter()
                .any(|key| frame.get(key).is_some())
        })
        .filter_map(|frame| frame.get("nodeId").and_then(Value::as_str))
        .collect::<std::collections::BTreeSet<_>>();
    let mut exported = Vec::new();
    let mut remaining = Vec::new();
    for group in groups {
        for frame in group["breakpoints"].as_array().into_iter().flatten() {
            let id = frame["nodeId"].as_str().unwrap_or_default();
            let included = exported_ids.contains(id)
                || (merged
                    && (all_screens
                        || selected.is_empty()
                        || selected.iter().any(|node| node == id)))
                || (result["source"]["nodeId"] == id && result.get("tsx").is_some());
            if included {
                exported.push(frame.clone());
            } else {
                remaining.push(frame.clone());
            }
        }
    }
    result.insert("breakpointCompletion".into(), json!({"exportedBreakpoints":exported,"remainingBreakpoints":remaining,
        "implementedBreakpoints":[],"implementationState":"unverified",
        "reportRequired":["implementedBreakpoints","remainingBreakpoints"],
        "guidance":"Implement and verify every exported width in the project, export remaining variants with responsiveTsx, and report both lists before calling the screen complete."}));
}

#[cfg(test)]
mod feedback_tests {
    use super::*;
    #[test]
    fn responsive_ambiguity_and_missing_widths_are_reviewed_within_selected_roots() {
        let snapshot:devup_mcp_figma::Snapshot=serde_json::from_value(json!({"fileKey":"Synthetic","roots":["1:1","1:2"],"nodes":{
            "1:1":{"id":"1:1","type":"FRAME","fields":{"name":"mobile","width":390,"childrenIds":["2:1","2:2"]}},
            "1:2":{"id":"1:2","type":"FRAME","fields":{"name":"desktop","width":1440,"childrenIds":["3:1"]}},
            "2:1":{"id":"2:1","type":"TEXT","fields":{"name":"Item"}},"2:2":{"id":"2:2","type":"TEXT","fields":{"name":"Item"}},
            "3:1":{"id":"3:1","type":"TEXT","fields":{"name":"Other"}},
            "9:1":{"id":"9:1","type":"FRAME","fields":{"childrenIds":["9:2","9:3"]}},
            "9:2":{"id":"9:2","type":"TEXT","fields":{"name":"unselected"}},"9:3":{"id":"9:3","type":"TEXT","fields":{"name":"unselected"}}
        },"diagnostics":[]})).unwrap();
        let report = review(&snapshot);
        assert!(
            report["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["kind"] == "ambiguous-sibling-correspondence")
        );
        assert!(!report.to_string().contains("unselected"));
        assert_eq!(width_evidence(&snapshot)["slots"][2]["state"], "undefined");
        assert_eq!(
            width_evidence(&snapshot)["slots"][4]["state"],
            "drawn-reference"
        );
    }
}
