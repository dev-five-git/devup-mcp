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
        let name = candidate.node.name.trim().to_ascii_lowercase();
        let family = if is_breakpoint_name(&name) {
            "@breakpoint-labels".into()
        } else {
            name
        };
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
