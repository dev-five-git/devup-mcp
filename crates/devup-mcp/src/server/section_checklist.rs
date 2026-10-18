//! Review prompts grounded in captured node fields, never screenshot perception.
use devup_mcp_figma::CollectedPayload;
use serde_json::{Value, json};

pub(super) fn checklist(payload: &CollectedPayload, roots: &[String]) -> Value {
    let snapshot = &payload.snapshot;
    let sections = roots
        .iter()
        .filter_map(|id| snapshot.nodes.get(id))
        .collect::<Vec<_>>();
    let mut records = Vec::new();
    for section in sections.iter().take(20) {
        let mut items = Vec::new();
        let mut queue = std::collections::VecDeque::from([section.id.as_str()]);
        let mut seen = std::collections::BTreeSet::new();
        let mut ids = Vec::new();
        while let Some(id) = queue.pop_front() {
            if !seen.insert(id) {
                continue;
            }
            if ids.len() >= 100 {
                queue.push_front(id);
                break;
            }
            ids.push(id);
            if let Some(node) = snapshot.nodes.get(id) {
                queue.extend(node.typed_view().child_ids());
            }
        }
        for id in &ids {
            let Some(node) = snapshot.nodes.get(*id) else {
                continue;
            };
            let view = node.typed_view();
            let children = view.child_ids().collect::<Vec<_>>();
            if view.string("layoutMode") == Some("HORIZONTAL") && children.len() >= 2 {
                items.push(json!({"kind":"child-count-and-spacing","nodeId":id,"name":view.name(),
                    "observed":{"capturedChildCount":children.len(),"gap":view.value("itemSpacing"),"padding":[view.value("paddingTop"),view.value("paddingRight"),view.value("paddingBottom"),view.value("paddingLeft")]},
                    "check":"Compare visible menu/item count and spacing with the implementation; hidden children and semantics require review.","state":"review-required"}));
            }
            let shapes = children
                .iter()
                .filter_map(|id| snapshot.nodes.get(*id))
                .filter(|n| {
                    matches!(
                        n.node_type.as_str(),
                        "VECTOR" | "ELLIPSE" | "LINE" | "POLYGON"
                    )
                })
                .collect::<Vec<_>>();
            if !shapes.is_empty() {
                items.push(json!({"kind":"decorative-shape-coverage","nodeId":id,
                    "evidence":shapes.iter().take(20).map(|n|json!({"nodeId":n.id,"type":n.node_type,"name":n.typed_view().name()})).collect::<Vec<_>>(),
                    "capturedShapeCount":shapes.len(),"check":"Review connector, arrow and ring coverage where these captured shapes are used; shape type does not establish its semantic role.","state":"review-required"}));
            }
            if view
                .value("fills")
                .and_then(Value::as_array)
                .is_some_and(|fills| {
                    fills
                        .iter()
                        .any(|p| p["type"] == "IMAGE" && p["visible"] != false)
                })
            {
                let evidence=payload.assets.iter().filter(|a|a.node_id==**id).map(|a|json!({"assetId":a.asset_id,"sourceSize":a.render_info.as_ref().map(|v|&v["sourceSize"]),"outputSize":a.render_info.as_ref().map(|v|&v["outputSize"]),"status":a.status})).collect::<Vec<_>>();
                items.push(json!({"kind":"image-resolution-and-sharp-layer","nodeId":id,"displayLogicalSize":super::compact::layout(node),
                    "acquiredEvidence":evidence,"check":"Compare acquired source pixels against logical display size and required density; check that intended sharp image layers accompany blur layers.",
                    "sourceResolutionState":if evidence.iter().any(|v|v["sourceSize"].is_object()) {"captured"} else {"unavailable"},"state":"review-required"}));
            }
        }
        records.push(json!({"nodeId":section.id,"name":section.typed_view().name(),"items":items.iter().take(30).collect::<Vec<_>>(),
            "totalItems":items.len(),"itemsTruncated":items.len()>30,"examinedNodes":ids.len().min(100),"nodeScanTruncated":!queue.is_empty()}));
    }
    json!({"sections":records,"totalSections":sections.len(),"sectionsTruncated":sections.len()>20,
        "scope":"selected captured roots with at most 100 descendants examined per root","implementationState":"unverified",
        "evidenceLimit":include_str!("text/section_checklist-checklist-500ab77e.md")})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_semantic_findings_without_captured_evidence() {
        let snapshot:devup_mcp_figma::Snapshot=serde_json::from_value(json!({"fileKey":"Synthetic","roots":["1:1"],"nodes":{"1:1":{"id":"1:1","type":"FRAME","fields":{"name":"Header","childrenIds":[]}}},"diagnostics":[]})).unwrap();
        let payload = CollectedPayload {
            target: devup_mcp_figma::FigmaTarget::bridge_current(),
            scope: devup_mcp_figma::CollectionScope::Node,
            metadata: Value::Null,
            snapshot,
            variables: None,
            styles: None,
            completeness: devup_mcp_figma::PayloadCompleteness::ResolvedValuesOnly,
            source_version: None,
            stats: Default::default(),
            assets: vec![],
            reference_png: None,
            failures: vec![],
        };
        assert_eq!(
            checklist(&payload, &["1:1".into()])["sections"][0]["items"],
            json!([])
        );
    }
    #[test]
    fn nested_sections_keep_spacing_shapes_and_unknown_image_resolution_evidence() {
        let payload:CollectedPayload=serde_json::from_value(json!({"target":{"fileKey":"Synthetic","nodeId":"1:1","branchKey":null},"scope":"node","metadata":{},"variables":null,"styles":null,"completeness":"resolved-values-only","sourceVersion":null,
            "snapshot":{"fileKey":"Synthetic","roots":["1:1"],"nodes":{
                "1:1":{"id":"1:1","type":"FRAME","fields":{"childrenIds":["1:2"]}},
                "1:2":{"id":"1:2","type":"SECTION","fields":{"childrenIds":["1:3"]}},
                "1:3":{"id":"1:3","type":"FRAME","fields":{"layoutMode":"HORIZONTAL","itemSpacing":18,"childrenIds":["1:4","1:5","1:6"]}},
                "1:4":{"id":"1:4","type":"VECTOR","fields":{}},"1:5":{"id":"1:5","type":"ELLIPSE","fields":{}},
                "1:6":{"id":"1:6","type":"RECTANGLE","fields":{"width":160,"height":90,"fills":[{"type":"IMAGE","imageHash":"synthetic"}]}}
            },"diagnostics":[]}})).unwrap();
        let result = checklist(&payload, &["1:1".into()]);
        let items = result["sections"][0]["items"].as_array().unwrap();
        for kind in [
            "child-count-and-spacing",
            "decorative-shape-coverage",
            "image-resolution-and-sharp-layer",
        ] {
            assert!(items.iter().any(|i| i["kind"] == kind), "{result}");
        }
        assert_eq!(
            items
                .iter()
                .find(|i| i["kind"] == "image-resolution-and-sharp-layer")
                .unwrap()["sourceResolutionState"],
            "unavailable"
        );
        assert_eq!(result["sections"][0]["examinedNodes"], 6);
    }
}
