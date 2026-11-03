//! Implementation handoff inferred from visual structure, kept outside executable TSX.
//!
//! A captured layout is evidence for a review, not proof that an animation or an
//! API binding exists. Every candidate retains that distinction through delivery.
use std::collections::{BTreeMap, BTreeSet};

use devup_mcp_figma::{RawNode, Snapshot};
use serde_json::{Value, json};

#[path = "behavior_contracts/detect.rs"]
mod detect;
#[cfg(test)]
#[path = "behavior_contracts/followup_tests.rs"]
mod followup_tests;
#[path = "behavior_contracts/motion_templates.rs"]
mod motion_templates;
#[path = "behavior_contracts/paging.rs"]
mod paging;
#[cfg(test)]
#[path = "behavior_contracts/tests.rs"]
mod tests;

const MAX_NODES: usize = 8_192;
const MAX_CANDIDATES: usize = 128;
const GUIDE: &str = "devup://guide/usage#behavior-and-data-contracts";

fn string<'a>(node: &'a RawNode, field: &str) -> &'a str {
    node.typed_view().string(field).unwrap_or("")
}

fn has(value: &str, labels: &[&str]) -> bool {
    let lower = value.to_lowercase();
    labels.iter().any(|label| lower.contains(label))
}

fn node_label(node: &RawNode) -> String {
    format!("{} {}", string(node, "name"), string(node, "characters"))
}

fn scoped_nodes<'a>(snapshot: &'a Snapshot, roots: &[String]) -> (Vec<&'a RawNode>, bool) {
    let mut pending = roots.to_vec();
    let mut visited = BTreeSet::new();
    let mut nodes = Vec::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        if nodes.len() == MAX_NODES {
            return (nodes, true);
        }
        if let Some(node) = snapshot.nodes.get(&id) {
            if node.typed_view().bool("visible") == Some(false) {
                continue;
            }
            pending.extend(node.typed_view().child_ids().map(str::to_owned));
            nodes.push(node);
        }
    }
    (nodes, false)
}

fn parents<'a>(nodes: &[&'a RawNode]) -> BTreeMap<&'a str, &'a RawNode> {
    let mut parents = BTreeMap::new();
    for node in nodes {
        for child in node.typed_view().child_ids() {
            parents.entry(child).or_insert(*node);
        }
    }
    parents
}

fn nearby_text(node: &RawNode, snapshot: &Snapshot, depth: usize) -> String {
    let mut text = node_label(node);
    if depth > 0 {
        for child in node.typed_view().child_ids().take(32) {
            if let Some(child) = snapshot.nodes.get(child)
                && child.typed_view().bool("visible") != Some(false)
            {
                text.push(' ');
                text.push_str(&nearby_text(child, snapshot, depth - 1));
            }
        }
    }
    text.chars().take(1_024).collect()
}

fn candidate(kind: &str, node: &RawNode, evidence: Value, implementation: Value) -> Value {
    let reactions = node
        .typed_view()
        .value("reactions")
        .and_then(Value::as_array)
        .map(|items| items.iter().take(16).cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    json!({
        "id":format!("{kind}:{}",node.id), "kind":kind, "nodeId":node.id,
        "name":string(node,"name"), "confidence":"heuristic",
        "classification":"implementation-recommendation", "evidence":evidence,
        "intentBasis":if reactions.is_empty(){"visual-inference"}else{"captured-prototype-plus-visual-inference"},
        "capturedPrototypeActions":reactions,
        "figmaAnimationVerified":false, "implemented":false, "runtimeVerified":false,
        "implementation":implementation, "guide":GUIDE
    })
}

fn roots(result: &Value, snapshot: &Snapshot) -> Vec<String> {
    if let Some(id) = result["nodeId"]
        .as_str()
        .or(result["source"]["nodeId"].as_str())
    {
        vec![id.into()]
    } else if let Some(frames) = result["frames"].as_array() {
        frames
            .iter()
            .filter_map(|frame| frame["nodeId"].as_str().map(str::to_owned))
            .collect()
    } else {
        snapshot.roots.clone()
    }
}

fn data_binding(node: &RawNode, role: &str, evidence: Value) -> Value {
    json!({
        "nodeId":node.id,"role":role,"evidence":evidence,
        "backendMatched":false,"apiAnchor":null,"databaseAnchor":null,
        "status":"source-required","literalIsOperationalData":false,
        "labelPolicy":"Keep fixed captions such as total/registered/applied as labels. Bind the numeric value and unit separately; a numeric value in a label-named node requires review, not automatic substitution.",
        "orderingPolicy":"Preserve captured display ordinals and the API's documented sort order. A descending 3/2/1 row label is presentation order, not a database primary key; do not replace it with an ID or infer ascending order.",
        "nextAction":"Read devup_project_context api/db, identify the owning fields, and reuse the same source in landing and detail views. Do not infer an endpoint or schema from this design.",
        "aggregation":"Compute totals and status/category breakdowns from the same source records; do not duplicate today's total in TSX or CMS.",
        "failurePolicy":"Propagate unavailable/malformed data; never return design fixtures as successful live data.",
        "verification":["Change records and values in an isolated DB and check every rendering","Empty data and backend outage","Preserve existing edits and verify idempotent backfills","Distinguish source wiring from API-to-browser and write-to-DB verification"]
    })
}

/// Add a bounded implementation plan to every requested code/frame delivery.
/// This never mutates TSX or asserts that recommendations have been implemented.
pub(super) fn attach(result: &mut Value, snapshot: &Snapshot) {
    if !result.is_object()
        || ![
            "tsx",
            "componentTsx",
            "responsiveTsx",
            "deliverable",
            "frames",
            "nodeId",
        ]
        .iter()
        .any(|field| result.get(*field).is_some())
    {
        return;
    }
    result["behaviorPlan"] = paging::page(result, snapshot, None).expect("initial behavior page");
    motion_templates::attach(result);
    let artifact = result["artifactId"].clone();
    if let Some(frames) = result.get_mut("frames").and_then(Value::as_array_mut) {
        for frame in frames {
            if artifact.is_string() {
                frame["artifactId"] = artifact.clone();
            }
            attach(frame, snapshot);
        }
    }
}

pub(super) fn resume(artifact: &str, snapshot: &Snapshot, cursor: &str) -> Result<Value, String> {
    let mut result = json!({"artifactId":artifact});
    result["behaviorPlan"] = paging::page(&result, snapshot, Some(cursor))?;
    motion_templates::attach(&mut result);
    Ok(result)
}

pub(super) fn verified_coverage_inventory(plan: &Value) -> Option<(Vec<Value>, Vec<String>)> {
    paging::verified_inventory(plan)
}
