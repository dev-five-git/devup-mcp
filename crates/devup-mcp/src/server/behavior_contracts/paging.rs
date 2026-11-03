use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

const MAX_CURSOR_BYTES: usize = 8 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Cursor {
    artifact: String,
    fingerprint: String,
    scope: Vec<String>,
    pending: Vec<String>,
    visited: BTreeSet<String>,
    candidates: Vec<Value>,
    bindings: Vec<Value>,
    emitted: BTreeSet<String>,
    emitted_bindings: BTreeSet<String>,
    palettes: Vec<Value>,
    palette_index: usize,
    evidence: Value,
    page: usize,
}

fn fingerprint(snapshot: &Snapshot) -> String {
    Sha256::digest(serde_json::to_vec(snapshot).expect("snapshot JSON"))
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn signature(body: &[u8]) -> String {
    static KEY: OnceLock<[u8; 32]> = OnceLock::new();
    let key = KEY.get_or_init(rand::random);
    let mut digest = Sha256::new();
    digest.update(key);
    digest.update(body);
    digest.update(key);
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn compact_strings(value: &mut Value, nodes: &BTreeMap<&str, usize>) {
    match value {
        Value::String(text) => {
            let compact = if let Some(index) = nodes.get(text.as_str()) {
                Some(format!("@n:{index}"))
            } else if let Some((kind, node)) = text.split_once(':')
                && let Some(index) = nodes.get(node)
            {
                Some(format!("@c:{kind}:{index}"))
            } else if let Some((node, role)) = text.rsplit_once("::")
                && let Some(index) = nodes.get(node)
            {
                Some(format!("@b:{index}:{role}"))
            } else {
                text.starts_with('@').then(|| format!("@s:{text}"))
            };
            if let Some(compact) = compact {
                *text = compact;
            }
        }
        Value::Array(values) => values
            .iter_mut()
            .for_each(|value| compact_strings(value, nodes)),
        Value::Object(values) => values
            .values_mut()
            .for_each(|value| compact_strings(value, nodes)),
        _ => {}
    }
}

fn expand_strings(value: &mut Value, nodes: &[&str]) -> Result<(), String> {
    let node = |index: &str| {
        index
            .parse::<usize>()
            .ok()
            .and_then(|index| nodes.get(index).copied())
            .ok_or_else(|| "Invalid compact behavior cursor node reference".to_owned())
    };
    match value {
        Value::String(text) => {
            let expanded = if let Some(literal) = text.strip_prefix("@s:") {
                Some(literal.to_owned())
            } else if let Some(index) = text.strip_prefix("@n:") {
                Some(node(index)?.to_owned())
            } else if let Some(rest) = text.strip_prefix("@c:") {
                let (kind, index) = rest
                    .rsplit_once(':')
                    .ok_or("Invalid compact candidate reference")?;
                Some(format!("{kind}:{}", node(index)?))
            } else if let Some(rest) = text.strip_prefix("@b:") {
                let (index, role) = rest
                    .split_once(':')
                    .ok_or("Invalid compact binding reference")?;
                Some(format!("{}::{role}", node(index)?))
            } else {
                None
            };
            if let Some(expanded) = expanded {
                *text = expanded;
            }
        }
        Value::Array(values) => {
            for value in values {
                expand_strings(value, nodes)?;
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                expand_strings(value, nodes)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn encode_with_limit(cursor: &Cursor, snapshot: &Snapshot, limit: usize) -> Result<String, usize> {
    let nodes = snapshot
        .nodes
        .keys()
        .enumerate()
        .map(|(index, id)| (id.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let mut value = serde_json::to_value(cursor).expect("cursor JSON");
    compact_strings(&mut value, &nodes);
    let bytes = serde_json::to_vec(&json!({"encoding":"snapshot-node-index-v1","cursor":value}))
        .expect("compact cursor JSON");
    let encoded_bytes = bytes.len() / 3 * 4
        + match bytes.len() % 3 {
            0 => 0,
            remainder => remainder + 1,
        }
        + 65;
    if encoded_bytes > limit {
        return Err(encoded_bytes);
    }
    Ok(format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(&bytes),
        signature(&bytes)
    ))
}

fn decode(token: &str, snapshot: &Snapshot) -> Result<Cursor, String> {
    if token.len() > MAX_CURSOR_BYTES {
        return Err("Behavior cursor exceeds the advertised 8 MiB bound. Export this artifact's rawSnapshot as a resource and restart explicitly with smaller original scopes.".into());
    }
    let (body, sig) = token.split_once('.').ok_or("Invalid behavior cursor")?;
    let bytes = URL_SAFE_NO_PAD
        .decode(body)
        .map_err(|_| "Invalid behavior cursor encoding")?;
    if signature(&bytes) != sig {
        return Err("Behavior cursor is invalid or belongs to a previous server process; restart this artifact scope.".into());
    }
    let mut value: Value =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid behavior cursor body")?;
    if value["encoding"] == "snapshot-node-index-v1" {
        value = value["cursor"].take();
        let nodes = snapshot
            .nodes
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        expand_strings(&mut value, &nodes)?;
    }
    serde_json::from_value(value).map_err(|_| "Invalid behavior cursor body".into())
}

fn continuation_action(cursor: &Cursor, snapshot: &Snapshot, limit: usize) -> (Value, bool) {
    match encode_with_limit(cursor, snapshot, limit) {
        Ok(token) => (
            json!({"tool":"devup_figma_export","arguments":{"artifactId":cursor.artifact,"behaviorCursor":token}}),
            false,
        ),
        Err(required) => (
            json!({"tool":"devup_figma_export","arguments":{"artifactId":cursor.artifact,"outputs":["rawSnapshot"],"debug":true,"delivery":"resource"},
            "reason":"Compact behavior continuation exceeds its accepted token budget; no unusable cursor or complete receipt was issued.",
            "requiredCursorBytes":required,"maxCursorBytes":limit,"originalScopeNodeIds":cursor.scope,
            "recovery":"Read the complete immutable snapshot resource and explicitly restart exports in smaller original scopes. Keep this inventory incomplete until the original scope is fully covered."}),
            true,
        ),
    }
}

pub(super) fn page(
    result: &Value,
    snapshot: &Snapshot,
    token: Option<&str>,
) -> Result<Value, String> {
    page_with_limit(result, snapshot, token, MAX_CURSOR_BYTES)
}

fn page_with_limit(
    result: &Value,
    snapshot: &Snapshot,
    token: Option<&str>,
    cursor_limit: usize,
) -> Result<Value, String> {
    let artifact = result["artifactId"].as_str().unwrap_or_default();
    let digest = fingerprint(snapshot);
    let mut cursor: Cursor = if let Some(token) = token {
        let cursor = decode(token, snapshot)?;
        if cursor.artifact != artifact || cursor.fingerprint != digest {
            return Err(
                "Behavior continuation must use the same immutable artifact and scope.".into(),
            );
        }
        cursor
    } else {
        let scope = roots(result, snapshot);
        let palettes=result["instanceOverrides"].as_array().into_iter().flatten().filter(|record|
            record["overriddenFields"].as_array().into_iter().flatten().filter_map(Value::as_str).any(|field|has(field,&["fill","stroke","color"]))
        ).map(|record|json!({"instanceNodeId":record["instanceNodeId"],"mainComponentId":record["mainComponentId"],"overriddenFields":record["overriddenFields"]})).collect();
        Cursor {
            artifact: artifact.into(),
            fingerprint: digest,
            pending: scope.clone(),
            scope,
            visited: BTreeSet::new(),
            candidates: Vec::new(),
            bindings: Vec::new(),
            emitted: BTreeSet::new(),
            emitted_bindings: BTreeSet::new(),
            palettes,
            palette_index: 0,
            evidence: json!({"interactiveCandidates":result["interactiveCandidates"]}),
            page: 0,
        }
    };
    let all = snapshot.nodes.values().collect::<Vec<_>>();
    let mut ancestry = parents(&all);
    for id in &cursor.scope {
        ancestry.remove(id.as_str());
    }
    let mut scanned = Vec::new();
    let mut out = Vec::new();
    let mut bindings = Vec::new();
    for _ in 0..MAX_NODES {
        while out.len() < MAX_CANDIDATES && !cursor.candidates.is_empty() {
            let candidate = cursor.candidates.remove(0);
            let id = candidate["id"].as_str().unwrap_or_default().to_owned();
            if cursor.emitted.insert(id) {
                out.push(candidate);
            }
        }
        while bindings.len() < MAX_CANDIDATES && !cursor.bindings.is_empty() {
            let binding = cursor.bindings.remove(0);
            let key = format!(
                "{}::{}",
                binding["nodeId"].as_str().unwrap_or_default(),
                binding["role"].as_str().unwrap_or_default()
            );
            if cursor.emitted_bindings.insert(key) {
                bindings.push(binding);
            }
        }
        if out.len() == MAX_CANDIDATES || bindings.len() == MAX_CANDIDATES {
            break;
        }
        if let Some(record) = cursor.palettes.get(cursor.palette_index) {
            detect::palette(
                &json!({"instanceOverrides":[record]}),
                snapshot,
                &mut cursor.candidates,
            );
            cursor.palette_index += 1;
            continue;
        }
        let Some(id) = cursor.pending.pop() else {
            break;
        };
        if !cursor.visited.insert(id.clone()) {
            continue;
        }
        let Some(node) = snapshot.nodes.get(&id) else {
            continue;
        };
        if node.typed_view().bool("visible") == Some(false)
            || node.typed_view().number("opacity") == Some(0.0)
        {
            continue;
        }
        cursor
            .pending
            .extend(node.typed_view().child_ids().map(str::to_owned));
        scanned.push(node);
        if let Some(count) = detect::statistic(node, snapshot, &ancestry) {
            cursor.bindings.push(data_binding(
                node,
                "presentation-statistic",
                count["evidence"].clone(),
            ));
            cursor.candidates.push(count);
        }
        cursor
            .candidates
            .extend(detect::marquee(node, snapshot, &ancestry, &cursor.evidence));
        cursor.candidates.extend(detect::progress(node, snapshot));
        cursor
            .candidates
            .extend(detect::tabs(node, snapshot, &cursor.evidence));
        cursor
            .candidates
            .extend(detect::view_switch(node, snapshot));
        cursor.candidates.extend(detect::section_entrance(
            node,
            snapshot,
            &cursor.scope,
            &ancestry,
        ));
        if detect::editable_content(node) {
            cursor.bindings.push(data_binding(
                node,
                "editable-business-content",
                json!({"name":string(node,"name")}),
            ));
        }
    }
    let contracts =
        detect::page_contracts(&cursor.scope, snapshot, &scanned, &mut cursor.candidates);
    while out.len() < MAX_CANDIDATES && !cursor.candidates.is_empty() {
        let candidate = cursor.candidates.remove(0);
        if cursor
            .emitted
            .insert(candidate["id"].as_str().unwrap_or_default().into())
        {
            out.push(candidate);
        }
    }
    while bindings.len() < MAX_CANDIDATES && !cursor.bindings.is_empty() {
        let binding = cursor.bindings.remove(0);
        let key = format!(
            "{}::{}",
            binding["nodeId"].as_str().unwrap_or_default(),
            binding["role"].as_str().unwrap_or_default()
        );
        if cursor.emitted_bindings.insert(key) {
            bindings.push(binding);
        }
    }
    out.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    let complete = cursor.pending.is_empty()
        && cursor.candidates.is_empty()
        && cursor.bindings.is_empty()
        && cursor.palette_index == cursor.palettes.len();
    let page_index = cursor.page;
    cursor.page += 1;
    let inventory = cursor
        .emitted
        .iter()
        .map(|id| {
            let (kind, node) = id.split_once(':').unwrap_or((id.as_str(), ""));
            json!({"id":id,"kind":kind,"nodeId":node})
        })
        .collect::<Vec<_>>();
    let receipt = json!({"artifactId":artifact,"snapshotFingerprint":cursor.fingerprint,"scopeNodeIds":cursor.scope,
        "complete":complete,"pageCount":cursor.page,"candidates":inventory,"bindingKeys":cursor.emitted_bindings});
    let receipt_signature = signature(&serde_json::to_vec(&receipt).expect("receipt JSON"));
    let (next_action, cursor_budget_exceeded) = if complete {
        (Value::Null, false)
    } else {
        continuation_action(&cursor, snapshot, cursor_limit)
    };
    Ok(
        json!({"version":2,"artifactId":artifact,"snapshotFingerprint":cursor.fingerprint,"scopeNodeIds":cursor.scope,"candidateCount":out.len(),"candidates":out,
        "aggregateCandidates":inventory,"aggregateBindingKeys":cursor.emitted_bindings,
        "coverageReceipt":{"payload":receipt,"signature":receipt_signature},
        "dataBindings":bindings,"pageContracts":contracts,"implemented":false,"runtimeVerified":false,"requiresReview":true,
        "pagination":{"pageIndex":page_index,"complete":complete,"accumulatedNodes":cursor.visited.len(),
            "aggregation":"Append candidates by id, bindings by nodeId+role and page contracts by kind+rootNodeId+footerNodeId; keep all pages until complete=true.",
            "cursorBudgetExceeded":cursor_budget_exceeded,"nextAction":next_action},
        "evidenceLimits":{"nodesScanned":scanned.len(),"maxNodes":MAX_NODES,"maxCandidates":MAX_CANDIDATES,"truncated":!complete,"contextIndex":"The immutable artifact graph and fingerprint are read for context; candidate inference and returned rows are bounded per page."},
        "intentPolicy":"Captured prototype evidence and inferred motion choices remain separate; explicit user requirements take precedence.",
        "clientBoundary":"Keep generated server TSX and data pages; mount the supplied tiny shared motion controller in the persistent layout.",
        "sourcePurity":"Use semantic selectors; node identities and implementation evidence stay in JSON sidecars.","guide":GUIDE}),
    )
}

pub(super) fn verified_inventory(plan: &Value) -> Option<(Vec<Value>, Vec<String>)> {
    let receipt = &plan["coverageReceipt"]["payload"];
    if plan["pagination"]["complete"] != true
        || receipt["complete"] != true
        || receipt["artifactId"] != plan["artifactId"]
        || receipt["snapshotFingerprint"] != plan["snapshotFingerprint"]
        || receipt["scopeNodeIds"] != plan["scopeNodeIds"]
        || receipt["candidates"] != plan["aggregateCandidates"]
        || receipt["bindingKeys"] != plan["aggregateBindingKeys"]
        || plan["coverageReceipt"]["signature"].as_str()?
            != signature(&serde_json::to_vec(receipt).ok()?)
    {
        return None;
    }
    Some((
        receipt["candidates"].as_array()?.clone(),
        receipt["bindingKeys"]
            .as_array()?
            .iter()
            .map(|key| key.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()?,
    ))
}

#[cfg(test)]
mod cursor_budget_tests {
    use super::*;

    #[test]
    fn compound_ids_larger_than_the_old_budget_resume_exactly_using_snapshot_indices() {
        let prefix = "I12:345;".repeat(1150);
        let ids = (0..1000)
            .map(|index| format!("{prefix}{index}:9"))
            .collect::<Vec<_>>();
        let mut nodes = vec![
            json!({"id":"root","type":"FRAME","fields":{"name":"Statistics","childrenIds":ids}}),
        ];
        nodes.extend(
            ids.iter().map(
                |id| json!({"id":id,"type":"TEXT","fields":{"name":"Metric","characters":"30"}}),
            ),
        );
        let snapshot = super::super::tests::snapshot(json!(nodes), &["root"]);
        let cursor = Cursor {
            artifact: "long".into(),
            fingerprint: fingerprint(&snapshot),
            scope: vec!["root".into()],
            pending: ids.clone(),
            visited: BTreeSet::from(["root".into()]),
            candidates: Vec::new(),
            bindings: Vec::new(),
            emitted: BTreeSet::new(),
            emitted_bindings: BTreeSet::new(),
            palettes: Vec::new(),
            palette_index: 0,
            evidence: json!({"literal":"@n:0"}),
            page: 0,
        };
        assert!(serde_json::to_vec(&cursor).unwrap().len() > MAX_CURSOR_BYTES);
        let token = encode_with_limit(&cursor, &snapshot, MAX_CURSOR_BYTES).unwrap();
        assert!(token.len() < 64 * 1024);
        let decoded = decode(&token, &snapshot).unwrap();
        assert_eq!(decoded.pending, ids);
        assert_eq!(decoded.evidence, cursor.evidence);
        let first = page(&json!({"artifactId":"long"}), &snapshot, Some(&token)).unwrap();
        assert_eq!(first["candidateCount"], MAX_CANDIDATES);
        let next = first["pagination"]["nextAction"]["arguments"]["behaviorCursor"]
            .as_str()
            .unwrap();
        assert!(next.len() < 64 * 1024);
        let second = page(&json!({"artifactId":"long"}), &snapshot, Some(next)).unwrap();
        assert_eq!(second["candidateCount"], MAX_CANDIDATES);
        assert!(!first["candidates"].as_array().unwrap().iter().any(|a| {
            second["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .any(|b| a["id"] == b["id"])
        }));
    }

    #[test]
    fn produced_budget_failure_stays_incomplete_and_returns_a_valid_recovery_operation() {
        let ids = (0..MAX_CANDIDATES + 2)
            .map(|index| format!("n{index}"))
            .collect::<Vec<_>>();
        let mut nodes = vec![
            json!({"id":"root","type":"FRAME","fields":{"name":"Statistics","childrenIds":ids}}),
        ];
        nodes.extend(
            ids.iter().map(
                |id| json!({"id":id,"type":"TEXT","fields":{"name":"Metric","characters":"30"}}),
            ),
        );
        let snapshot = super::super::tests::snapshot(json!(nodes), &["root"]);
        let plan = page_with_limit(
            &json!({"artifactId":"bounded","tsx":"<Box/>"}),
            &snapshot,
            None,
            64,
        )
        .unwrap();
        assert_eq!(plan["pagination"]["complete"], false);
        assert_eq!(plan["pagination"]["cursorBudgetExceeded"], true);
        assert!(verified_inventory(&plan).is_none());
        let action = &plan["pagination"]["nextAction"];
        assert_eq!(action["tool"], "devup_figma_export");
        assert!(action["arguments"].get("behaviorCursor").is_none());
        assert_eq!(action["arguments"]["artifactId"], "bounded");
        assert_eq!(action["arguments"]["delivery"], "resource");
        let outputs = action["arguments"]["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        super::super::super::validation::validate_outputs(
            &outputs,
            action["arguments"]["debug"] == true,
        )
        .unwrap();
        assert!(action["requiredCursorBytes"].as_u64().unwrap() > 64);
    }
}
