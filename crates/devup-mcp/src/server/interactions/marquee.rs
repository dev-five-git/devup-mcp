use super::*;
use devup_mcp_devup_ui::ui_validate::visual_behavior::{MARQUEE_RECIPE, repeated_separator};

pub(super) fn candidate(snapshot: &Snapshot, node: &RawNode) -> Option<InteractiveCandidate> {
    let view = node.typed_view();
    let children = view.child_ids().filter_map(|id| snapshot.nodes.get(id))
        .filter(|child| child.typed_view().bool("visible") != Some(false)).collect::<Vec<_>>();
    let row = view.string("layoutMode") == Some("HORIZONTAL")
        && view.string("layoutWrap") != Some("WRAP")
        && children.len() >= 3
        && children.iter().all(|child| child.node_type == "TEXT")
        && children.iter().filter_map(|child| child.typed_view().string("characters"))
            .collect::<BTreeSet<_>>().len() < children.len();
    if node.node_type != "TEXT" && !row { return None }
    let text = if row {
        children.iter().filter_map(|child| child.typed_view().string("characters")).collect::<Vec<_>>().join(" ")
    } else { str_field(node, "characters").to_owned() };
    if text.trim().is_empty() || text.contains(['\n', '\r']) { return None }
    let single_line = row || view.string("textAutoResize") == Some("WIDTH_AND_HEIGHT")
        || view.number("maxLines") == Some(1.0)
        || matches!((view.number("height"), view.number("fontSize")), (Some(height), Some(size)) if height <= size * 1.6);
    let width = view.number("width");
    let mut current = node;
    let mut visited = BTreeSet::new();
    let mut clip = None;
    while visited.insert(current.id.as_str()) {
        let parent = current.typed_view().string("parentId")
            .and_then(|id| snapshot.nodes.get(id))
            .or_else(|| snapshot.nodes.values().find(|parent| parent.typed_view().child_ids().any(|id| id == current.id)));
        let Some(parent) = parent else { break };
        let parent_view = parent.typed_view();
        if parent_view.bool("clipsContent") == Some(true)
            && matches!((width, parent_view.number("width")), (Some(width), Some(clip_width)) if width > clip_width + 1.0)
        {
            clip = Some(parent.id.clone());
            break;
        }
        current = parent;
    }
    let separators = repeated_separator(&text);
    if !(separators || single_line && clip.is_some()) { return None }
    Some(InteractiveCandidate {
        node_id: node.id.clone(), name: str_field(node, "name").into(),
        text: text.chars().take(500).collect(), role: Role::Marquee,
        reason: [separators.then_some("repeated-separator-pattern"),
            (single_line && clip.is_some()).then_some("single-line-text-exceeds-clipping-ancestor")]
            .into_iter().flatten().map(str::to_owned).collect(),
        has_prototype_target: false, target_resolved: false,
        unresolved_reasons: vec!["animation-intent-is-heuristic-not-figma-animation-data".into()],
        prototype_actions: Vec::new(),
        states: json!({"confidence":"heuristic", "clippingAncestorId":clip,
            "runtimeStateVerified":false,"recipe":MARQUEE_RECIPE,
            "animation":{"from":"translateX(0)","to":"translateX(-50%)","iterationCount":"infinite","timingFunction":"linear","reducedMotion":"none","duplicateAriaHidden":true}}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(nodes: Value) -> Snapshot {
        Snapshot { file_key: "fixture".into(), version: None, roots: vec!["clip".into()],
            nodes: nodes.as_array().unwrap().iter().map(|node| {
                let node: RawNode = serde_json::from_value(node.clone()).unwrap();
                (node.id.clone(), node)
            }).collect(), diagnostics: Vec::new() }
    }

    #[test]
    fn marquee_candidates_report_overflow_through_intermediate_ancestors() {
        let snapshot = snapshot(json!([
            {"id":"clip","type":"FRAME","fields":{"width":400,"clipsContent":true,"childrenIds":["track"]}},
            {"id":"track","type":"FRAME","fields":{"childrenIds":["text"]}},
            {"id":"text","type":"TEXT","fields":{"width":1200,"textAutoResize":"WIDTH_AND_HEIGHT","characters":"An overflowing research strip without separator glyphs"}}
        ]));
        let output = evidence(&snapshot, &snapshot.roots);
        let candidate = &output["interactiveCandidates"][0];
        assert_eq!(candidate["role"], "marquee");
        assert_eq!(candidate["states"]["confidence"], "heuristic");
        assert_eq!(candidate["states"]["clippingAncestorId"], "clip");
        assert_eq!(candidate["states"]["animation"]["duplicateAriaHidden"], true);
    }

    #[test]
    fn marquee_candidates_detect_separators_and_repeated_rows() {
        let snapshot = snapshot(json!([
            {"id":"clip","type":"FRAME","fields":{"width":400,"clipsContent":true,"childrenIds":["text","row"]}},
            {"id":"text","type":"TEXT","fields":{"characters":"Research ✦ Repair ✦ Renew ✦ Research"}},
            {"id":"row","type":"FRAME","fields":{"width":600,"layoutMode":"HORIZONTAL","childrenIds":["a","b","c"]}},
            {"id":"a","type":"TEXT","fields":{"characters":"Research","width":200}},
            {"id":"b","type":"TEXT","fields":{"characters":"Research","width":200}},
            {"id":"c","type":"TEXT","fields":{"characters":"Research","width":200}}
        ]));
        assert!(candidate(&snapshot, &snapshot.nodes["text"]).is_some());
        assert!(candidate(&snapshot, &snapshot.nodes["row"]).is_some());
        let mut wrapped = snapshot;
        wrapped.nodes.get_mut("row").unwrap().fields.insert("layoutWrap".into(), json!("WRAP"));
        assert!(candidate(&wrapped, &wrapped.nodes["row"]).is_none());
    }

    #[test]
    fn marquee_candidates_ignore_wrapped_unclipped_and_short_text() {
        let snapshot = snapshot(json!([
            {"id":"clip","type":"FRAME","fields":{"width":400,"childrenIds":["text"]}},
            {"id":"text","type":"TEXT","fields":{"width":1200,"textAutoResize":"HEIGHT","characters":"Research ✦ Repair\n✦ Renew ✦ Research"}}
        ]));
        assert!(candidate(&snapshot, &snapshot.nodes["text"]).is_none());
    }
}
