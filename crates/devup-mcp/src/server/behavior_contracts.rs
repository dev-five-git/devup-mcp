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
    let mut pending = roots.iter().map(String::as_str).collect::<Vec<_>>();
    let mut visited = BTreeSet::new();
    let mut nodes = Vec::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        if nodes.len() == MAX_NODES {
            return (nodes, true);
        }
        let Some(node) = snapshot.nodes.get(id) else {
            continue;
        };
        if node.typed_view().bool("visible") == Some(false)
            || node.typed_view().number("opacity") == Some(0.0)
        {
            continue;
        }
        pending.extend(node.typed_view().child_ids());
        nodes.push(node);
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
    let scope = roots(result, snapshot);
    let (nodes, node_limit) = scoped_nodes(snapshot, &scope);
    let parents = parents(&nodes);
    let mut candidates = Vec::new();
    let mut bindings = Vec::new();
    for node in &nodes {
        if let Some(count) = detect::statistic(node, snapshot, &parents) {
            bindings.push(data_binding(
                node,
                "presentation-statistic",
                count["evidence"].clone(),
            ));
            candidates.push(count);
        }
        if let Some(marquee) = detect::marquee(node, snapshot, &parents, result) {
            candidates.push(marquee);
        }
        if let Some(progress) = detect::progress(node, snapshot) {
            candidates.push(progress);
        }
        if let Some(tabs) = detect::tabs(node, snapshot, result) {
            candidates.push(tabs);
        }
        if let Some(view) = detect::view_switch(node, snapshot) {
            candidates.push(view);
        }
        if detect::editable_content(node) {
            bindings.push(data_binding(
                node,
                "editable-business-content",
                json!({"name":string(node,"name")}),
            ));
        }
    }
    detect::palette(result, snapshot, &mut candidates);
    let page_contracts = detect::page_contracts(&scope, snapshot, &nodes, &mut candidates);
    candidates.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    candidates.dedup_by(|a, b| a["id"] == b["id"]);
    let candidate_limit = candidates.len() > MAX_CANDIDATES;
    let binding_limit = bindings.len() > MAX_CANDIDATES;
    candidates.truncate(MAX_CANDIDATES);
    bindings.truncate(MAX_CANDIDATES);
    result["behaviorPlan"] = json!({
        "version":1,"scopeNodeIds":scope,"candidateCount":candidates.len(),
        "candidates":candidates,"dataBindings":bindings,"pageContracts":page_contracts,
        "implemented":false,"runtimeVerified":false,"requiresReview":true,
        "evidenceLimits":{"nodesScanned":nodes.len(),"maxNodes":MAX_NODES,"maxCandidates":MAX_CANDIDATES,"truncated":node_limit||candidate_limit||binding_limit},
        "intentPolicy":"Visual shape and repeated structure suggest behavior; they do not establish Figma motion or business semantics. Follow explicit user requirements and captured interactions; label all chosen timing and inferred behavior as implementation decisions.",
        "clientBoundary":"Preserve generated responsive TSX and Server Component layouts/data pages. Add only small shared client controllers for actual browser state or animation.",
        "sourcePurity":"Keep this plan, node identities and override evidence in JSON sidecars. Do not add metadata comments or source-node attributes to application TSX.",
        "verificationRequirements":["Match content, selected background, text, icon and ARIA to one normalized state","Test both directions, pointer and keyboard, direct URL, reload, history and resize","Test mobile/desktop and actual CSS viewport/zoom; compare geometry after accounting for viewport differences","Keep SSR and no-JS content readable; honor reduced motion and focus","Report candidates, wired behavior and browser-verified behavior separately"],
        "guide":GUIDE
    });
    if let Some(frames) = result.get_mut("frames").and_then(Value::as_array_mut) {
        for frame in frames {
            attach(frame, snapshot);
        }
    }
}
