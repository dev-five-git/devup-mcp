//! Design interaction evidence. Candidates are review hints, never invented links.
use std::collections::{BTreeMap, BTreeSet};

use devup_mcp_figma::{RawNode, Snapshot};
use serde::Serialize;
use serde_json::{Value, json};

const MAX_CANDIDATES: usize = 128;
const MAX_DETAILS: usize = 64;
fn bounded(value: &Value, depth: usize, truncated: &mut bool) -> Value {
    if depth == 0 && (value.is_object() || value.is_array()) {
        *truncated = true;
        return json!("omitted: nested detail cap");
    }
    match value {
        Value::String(s) if s.chars().count() > 160 => {
            *truncated = true;
            json!(s.chars().take(160).collect::<String>())
        }
        Value::Array(a) => {
            if a.len() > 16 {
                *truncated = true
            }
            Value::Array(
                a.iter()
                    .take(16)
                    .map(|v| bounded(v, depth - 1, truncated))
                    .collect(),
            )
        }
        Value::Object(o) => {
            if o.len() > 16 {
                *truncated = true
            }
            Value::Object(
                o.iter()
                    .take(16)
                    .map(|(k, v)| (k.clone(), bounded(v, depth - 1, truncated)))
                    .collect(),
            )
        }
        _ => value.clone(),
    }
}

#[derive(Debug, Clone, Copy, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum Role {
    Link,
    Button,
    Tab,
    Nav,
    Checkbox,
    Radio,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct InteractiveCandidate {
    pub node_id: String,
    pub name: String,
    pub text: String,
    pub role: Role,
    pub reason: Vec<String>,
    pub has_prototype_target: bool,
    /// A design target alone never resolves a code route or a record ID.
    pub target_resolved: bool,
    pub unresolved_reasons: Vec<String>,
    pub prototype_actions: Vec<Value>,
    pub states: Value,
}

pub(super) fn output_schema() -> std::sync::Arc<serde_json::Map<String, Value>> {
    let mut candidate = serde_json::to_value(schemars::schema_for!(InteractiveCandidate))
        .expect("candidate schema");
    let mut definitions = candidate
        .as_object_mut()
        .unwrap()
        .remove("$defs")
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    candidate.as_object_mut().unwrap().remove("$schema");
    // Value's schemars schema is a bare true; MCP clients require objects at
    // every schema position. These values are structured action/state objects.
    candidate["properties"]["prototypeActions"]["items"] =
        json!({"type":"object","additionalProperties":{}});
    candidate["properties"]["states"] = json!({"type":"object","additionalProperties":{}});
    definitions.insert("InteractiveCandidate".into(), candidate);
    std::sync::Arc::new(json!({"type":"object","additionalProperties":{},"properties":{"interactiveCandidates":{"type":"array","items":{"$ref":"#/$defs/InteractiveCandidate"}}},"$defs":definitions}).as_object().unwrap().clone())
}

fn str_field<'a>(node: &'a RawNode, field: &str) -> &'a str {
    node.typed_view().string(field).unwrap_or("")
}

fn has(label: &str, words: &[&str]) -> bool {
    let label = label.to_lowercase();
    words.iter().any(|word| label.contains(word))
}

pub(super) fn label_role(label: &str) -> Option<Role> {
    if has(label, &["checkbox", "check box", "체크박스", "동의"]) {
        Some(Role::Checkbox)
    } else if has(label, &["radio", "라디오"]) {
        Some(Role::Radio)
    } else if has(label, &["탭"])
        || label
            .to_lowercase()
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|s| ["tab", "tabs", "tabitem"].contains(&s))
    {
        Some(Role::Tab)
    } else if has(
        label,
        &["gnb", "navigation", "navbar", "breadcrumb", "메뉴", "menu"],
    ) {
        Some(Role::Nav)
    } else if has(
        label,
        &[
            "link",
            "로고",
            "logo",
            "개인정보",
            "이용약관",
            "privacy",
            "terms",
            "portfolio card",
            "portfoliocard",
            "카드",
            "breadcrumb",
        ],
    ) {
        Some(Role::Link)
    } else if has(
        label,
        &[
            "button",
            "btn",
            "cta",
            "견적",
            "문의하기",
            "전체 보기",
            "전체보기",
            "사례 보기",
            "view all",
            "arrow",
            "chevron",
            "화살표",
            "carousel",
        ],
    ) {
        Some(Role::Button)
    } else if [
        "소개",
        "서비스",
        "포트폴리오",
        "고객센터",
        "seo · geo",
        "seo/geo",
    ]
    .contains(&label.trim().to_lowercase().as_str())
    {
        Some(Role::Link)
    } else {
        None
    }
}

fn descendants(snapshot: &Snapshot, root: &str) -> Vec<String> {
    let mut pending = vec![root.to_owned()];
    let mut visited = BTreeSet::new();
    let mut result = vec![];
    while let Some(id) = pending.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        let Some(node) = snapshot.nodes.get(&id) else {
            continue;
        };
        if node.typed_view().bool("visible") == Some(false) {
            continue;
        }
        pending.extend(node.typed_view().child_ids().map(str::to_owned));
        result.push(id);
    }
    result
}

fn node_text(snapshot: &Snapshot, id: &str) -> String {
    descendants(snapshot, id)
        .iter()
        .filter_map(|id| snapshot.nodes[id].typed_view().string("characters"))
        .take(40)
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(500)
        .collect()
}

/// Both Plugin API reactions.actions and older reaction.action are captured.
fn actions(node: &RawNode) -> Vec<Value> {
    let mut result = vec![];
    for reaction in node
        .typed_view()
        .value("reactions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if reaction["trigger"]["type"] == "AFTER_TIMEOUT" {
            continue;
        }
        for action in reaction
            .get("action")
            .into_iter()
            .chain(reaction["actions"].as_array().into_iter().flatten())
        {
            let kind = action["type"].as_str().unwrap_or("UNKNOWN");
            let navigation = action["navigation"].as_str();
            result.push(json!({"trigger":reaction["trigger"]["type"],"type":kind,
                "navigation":navigation,"destinationId":action["destinationId"],"url":action["url"],
                "codeTargetResolved":false}));
        }
    }
    result
}

pub(super) fn candidates(snapshot: &Snapshot, roots: &[String]) -> Vec<InteractiveCandidate> {
    let ids: BTreeSet<_> = roots
        .iter()
        .flat_map(|id| descendants(snapshot, id))
        .collect();
    let mut result = vec![];
    for id in &ids {
        let node = &snapshot.nodes[id];
        let name = str_field(node, "name");
        let text = node_text(snapshot, id);
        let prototype_actions = actions(node);
        let explicit = label_role(name).or_else(|| view_component_role(node));
        let text_role = label_role(&text);
        let view = node.typed_view();
        let boxed = view.number("cornerRadius").is_some_and(|r| r > 0.0)
            && view
                .value("fills")
                .and_then(Value::as_array)
                .is_some_and(|f| !f.is_empty())
            && !text.is_empty()
            && text.chars().count() <= 35;
        // Text leaves inside a named control are not separate controls.
        let named_parent = ids.iter().any(|parent| {
            snapshot.nodes[parent]
                .typed_view()
                .child_ids()
                .any(|child| child == id)
                && label_role(str_field(&snapshot.nodes[parent], "name"))
                    .is_some_and(|role| !matches!(role, Role::Nav))
        });
        let role = explicit
            .or_else(|| {
                (!named_parent && node.node_type == "TEXT")
                    .then_some(text_role)
                    .flatten()
            })
            .or_else(|| boxed.then_some(Role::Button))
            .or_else(|| (!prototype_actions.is_empty()).then_some(Role::Button));
        let Some(mut role) = role else { continue };
        if !prototype_actions.is_empty()
            && prototype_actions
                .iter()
                .any(|a| a["url"].is_string() || a["navigation"] == "NAVIGATE")
        {
            role = Role::Link;
        }
        let has_target = prototype_actions
            .iter()
            .any(|a| a["url"].is_string() || a["destinationId"].is_string());
        let mut reason = vec![];
        if explicit.is_some() {
            reason.push("semantic-layer-or-instance-name".into());
        }
        if !named_parent && text_role.is_some() {
            reason.push("action-label".into());
        }
        if boxed {
            reason.push("short-text-background-rounded-box".into());
        }
        if !prototype_actions.is_empty() {
            reason.push("prototype-reaction".into());
        }
        let mut unresolved = vec!["static-export-does-not-wire-events".into()];
        if has_target {
            unresolved
                .push("prototype-target-needs-explicit-code-route-or-behavior-mapping".into());
        } else {
            unresolved.push("destination-or-action-not-declared".into());
        }
        if has(name, &["card", "카드", "portfolio", "포트폴리오"]) {
            unresolved.push("record-id-data-not-verified".into());
        }
        let properties = view
            .value("componentProperties")
            .cloned()
            .unwrap_or(json!({}));
        let variants = view
            .value("variantProperties")
            .cloned()
            .unwrap_or(json!({}));
        let mut truncated = false;
        let properties = bounded(&properties, 3, &mut truncated);
        let variants = bounded(&variants, 3, &mut truncated);
        result.push(InteractiveCandidate {
            node_id: id.clone(),
            name: name.into(),
            text,
            role,
            reason,
            has_prototype_target: has_target,
            target_resolved: false,
            unresolved_reasons: unresolved,
            prototype_actions,
            states: json!({"componentProperties":properties,"variantProperties":variants,
                "runtimeStateVerified":false,"captureTruncated":truncated}),
        });
    }
    result
}

fn view_component_role(node: &RawNode) -> Option<Role> {
    node.typed_view()
        .string("mainComponentName")
        .and_then(label_role)
        .or_else(|| {
            node.typed_view()
                .value("mainComponent")
                .and_then(|v| v["name"].as_str())
                .and_then(label_role)
        })
}

fn widget_composition(snapshot: &Snapshot, node: &RawNode) -> Option<&'static str> {
    let view = node.typed_view();
    let text = node_text(snapshot, &node.id).to_lowercase();
    if ["card number", "카드 번호", "mm/yy", "cvc", "cvv"]
        .iter()
        .filter(|s| text.contains(**s))
        .count()
        >= 2
    {
        return Some("payment-field-composition");
    }
    let children = view
        .child_ids()
        .filter_map(|id| snapshot.nodes.get(id))
        .collect::<Vec<_>>();
    let small_square=children.iter().any(|child| {
        let v=child.typed_view();
        matches!((v.number("width"),v.number("height")),(Some(w),Some(h)) if (15.0..=35.0).contains(&w) && (w-h).abs()<=2.0)
    });
    if matches!((view.number("width"),view.number("height")),(Some(w),Some(h)) if (250.0..=400.0).contains(&w) && (60.0..=100.0).contains(&h))
        && small_square
        && children.iter().any(|c| c.node_type == "TEXT")
        && children.len() >= 3
    {
        return Some("captcha-sized-panel-square-control-text-and-mark-composition");
    }
    if children.len() >= 3
        && children
            .iter()
            .any(|c| has(str_field(c, "name"), &["pin", "marker", "마커"]))
        && children.iter().any(|c| c.node_type == "VECTOR")
        && view.number("width").is_some_and(|w| w >= 200.0)
    {
        return Some("map-sized-panel-marker-and-vector-composition");
    }
    None
}

fn append_hints(result: &mut Value, data: &Value) {
    let mut hints = String::new();
    for candidate in result_candidates(data) {
        let hint = json!({"nodeId":candidate["nodeId"],"role":candidate["role"],"prototypeActions":candidate["prototypeActions"],"states":candidate["states"],"targetResolved":false});
        hints.push_str("\n// @devup-interaction ");
        hints.push_str(
            &hint
                .to_string()
                .replace('\u{2028}', "\\u2028")
                .replace('\u{2029}', "\\u2029"),
        );
    }
    for field in ["tsx", "componentTsx", "responsiveTsx"] {
        if let Some(code) = result.get(field).and_then(Value::as_str).map(str::to_owned) {
            result[field] = json!(format!("{code}{hints}"));
        }
    }
}

pub(super) fn evidence(snapshot: &Snapshot, roots: &[String]) -> Value {
    let mut candidates = candidates(snapshot, roots);
    let total_candidates = candidates.len();
    candidates.truncate(MAX_CANDIDATES);
    let ids: BTreeSet<_> = roots
        .iter()
        .flat_map(|r| descendants(snapshot, r))
        .collect();
    let mut masters: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut widgets = vec![];
    let mut details = vec![];
    let mut read_errors = vec![];
    for id in ids {
        let node = &snapshot.nodes[&id];
        let view = node.typed_view();
        let name = str_field(node, "name");
        for (field, error) in &node.field_errors {
            if [
                "reactions",
                "componentProperties",
                "variantProperties",
                "mainComponentId",
                "mainComponentSetId",
            ]
            .contains(&field.as_str())
            {
                read_errors.push(json!({"nodeId":id,"field":field,"reason":error}));
            }
        }
        if let Some(master) = view
            .string("mainComponentSetId")
            .or_else(|| view.string("mainComponentId"))
            .or_else(|| view.string("componentId"))
            .or_else(|| view.value("mainComponent").and_then(|v| v["id"].as_str()))
        {
            let labels = descendants(snapshot, &id)
                .iter()
                .filter_map(|id| snapshot.nodes[id].typed_view().string("characters"))
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let mut truncated = false;
            masters.entry(master.into()).or_default().push(json!({"nodeId":id,"name":name,"labelCount":labels.len(),"labels":labels.iter().take(24).collect::<Vec<_>>(),
                "text":node_text(snapshot,&id),"variantProperties":view.value("variantProperties").map(|v|bounded(v,3,&mut truncated)),"componentProperties":view.value("componentProperties").map(|v|bounded(v,3,&mut truncated)),"detailTruncated":truncated || labels.len()>24}));
        }
        let named_widget = has(
            name,
            &[
                "recaptcha",
                "captcha",
                "map widget",
                "google map",
                "payment",
                "stripe",
                "카카오맵",
                "지도",
                "결제",
            ],
        );
        let composition = widget_composition(snapshot, node);
        if named_widget || composition.is_some() {
            widgets.push(json!({"nodeId":id,"name":name,"reason":if named_widget{"widget-layer-name"}else{composition.unwrap()},"verified":false,
                "requiredAction":"Confirm provider, integration, consent and env key names; never invent keys or copy a widget screenshot as functionality."}));
        }
        if has(
            name,
            &[
                "icon",
                "chevron",
                "check",
                "chip",
                "checkbox",
                "아이콘",
                "칩",
                "체크",
            ],
        ) {
            let mut fields = serde_json::Map::new();
            let mut truncated = false;
            for field in [
                "cornerRadius",
                "rectangleCornerRadii",
                "strokes",
                "strokeWeight",
                "fills",
                "paddingLeft",
                "paddingRight",
                "paddingTop",
                "paddingBottom",
                "variantProperties",
                "componentProperties",
            ] {
                if let Some(value) = view.value(field) {
                    fields.insert(field.into(), bounded(value, 4, &mut truncated));
                }
            }
            details.push(json!({"nodeId":id,"name":name,"fields":fields,"fieldErrors":node.field_errors,"detailTruncated":truncated,
                "iconStyle":if has(name,&["outline","outlined","line"]){"outline-name-hint"}else if has(name,&["filled","solid"]){"filled-name-hint"}else{"unresolved"},
                "shapeAndStateVerified":false}));
        }
    }
    let mut shared = masters.into_iter().filter(|(_,members)| members.len()>1).map(|(master,mut members)| {
        let signatures: BTreeSet<_> = members.iter().map(|m| json!([m["text"],m["variantProperties"],m["componentProperties"]]).to_string()).collect();
        let total=members.len(); members.truncate(24);
        json!({"componentId":master,"instances":members,"instanceCount":total,"instancesOmitted":total-members.len(),"variantDifferences":signatures.len()>1,
            "requiredAction":"Reuse one shared component and wire links once; confirm label/menu/state differences before applying a version across screens."})
    }).collect::<Vec<_>>();
    let truncated = total_candidates > candidates.len()
        || widgets.len() > MAX_DETAILS
        || details.len() > MAX_DETAILS
        || shared.len() > MAX_DETAILS;
    let limits = json!({"truncated":truncated,"omittedCandidates":total_candidates-candidates.len(),"omittedWidgets":widgets.len().saturating_sub(MAX_DETAILS),"omittedStateDetails":details.len().saturating_sub(MAX_DETAILS),"omittedSharedGroups":shared.len().saturating_sub(MAX_DETAILS),"maxCandidates":MAX_CANDIDATES,"maxDetails":MAX_DETAILS,"maxInstancesPerGroup":24,"stateFieldCaps":{"maxEntries":16,"maxStringChars":160,"maxDepth":3}});
    widgets.truncate(MAX_DETAILS);
    details.truncate(MAX_DETAILS);
    shared.truncate(MAX_DETAILS);
    let mut evidence = json!({"interactiveCandidates":candidates,"sharedComponentCandidates":shared,"widgetCandidates":widgets,"stateAndIconDetails":details,
        "interactionEvidenceLimits":limits,
        "interactionReadErrors":read_errors,"stateProjection":"Variants/prototype actions appear in @devup-interaction TSX comments and structured metadata. They do not implement runtime state/events or add native semantics automatically.",
        "sharedComponentLimitations":"Only explicit master/set identity is grouped. Old snapshots without identity cannot establish shared components; no name match is treated as proof.",
        "interactionCoverage":{"candidateCount":total_candidates,"returnedCandidateCount":candidates.len(),"wiredCount":0,"runtimeVerified":false,"scope":"Design candidates only; code wiring must be measured after implementation."}});
    // Metadata stays bounded even when a capture contains large component text properties.
    evidence["interactionEvidenceLimits"]["maxBytes"] = json!(65_536);
    while evidence.to_string().len() > 65_536 {
        let field = [
            "interactiveCandidates",
            "sharedComponentCandidates",
            "widgetCandidates",
            "stateAndIconDetails",
            "interactionReadErrors",
        ]
        .into_iter()
        .filter(|f| evidence[*f].as_array().is_some_and(|a| !a.is_empty()))
        .max_by_key(|f| evidence[*f].to_string().len());
        let Some(field) = field else { break };
        evidence[field].as_array_mut().unwrap().pop();
        evidence["interactionEvidenceLimits"]["truncated"] = json!(true);
        let count = evidence["interactionEvidenceLimits"]["byteCapOmissions"][field]
            .as_u64()
            .unwrap_or(0);
        evidence["interactionEvidenceLimits"]["byteCapOmissions"][field] = json!(count + 1);
    }
    evidence["interactionCoverage"]["returnedCandidateCount"] =
        json!(result_candidates(&evidence).len());
    evidence
}

pub(super) fn attach(result: &mut Value, snapshot: &Snapshot) {
    let has_code = result.get("deliverable").is_some()
        || result.get("tsx").is_some()
        || result.get("frames").is_some();
    if !has_code {
        return;
    }
    let roots = if let Some(frames) = result["frames"].as_array() {
        frames
            .iter()
            .filter_map(|f| f["nodeId"].as_str().map(str::to_owned))
            .collect()
    } else if result.get("responsiveTsx").is_none()
        && let Some(node) = result["source"]["nodeId"].as_str()
    {
        vec![node.to_owned()]
    } else {
        snapshot.roots.clone()
    };
    let data = evidence(snapshot, &roots);
    let count = data["interactiveCandidates"].as_array().map_or(0, Vec::len);
    append_hints(result, &data);
    result
        .as_object_mut()
        .unwrap()
        .extend(data.as_object().unwrap().clone());
    if data["interactionEvidenceLimits"]["truncated"] == true {
        result.as_object_mut().unwrap().entry("projectionIssues").or_insert(json!([])).as_array_mut().unwrap().push(json!({"code":"DEVUP_CODEGEN_INTERACTION_TRUNCATED","severity":"warning","fidelityImpact":"none","message":"Interaction evidence exceeded a named cap; candidate coverage is incomplete.","details":{"classification":"interaction-unverified","nextAction":"Read interactionEvidenceLimits and narrow the export selection or inspect the raw snapshot before claiming all candidates resolved."}}));
        if result["deliverable"].is_object() {
            result["deliverable"]["isFinal"] = json!(false);
            result["deliverable"]["note"] = json!(
                "Interaction evidence is truncated; review the named caps before accepting completeness."
            );
        }
    }
    if data["interactionReadErrors"]
        .as_array()
        .is_some_and(|a| !a.is_empty())
    {
        result.as_object_mut().unwrap().entry("projectionIssues").or_insert(json!([])).as_array_mut().unwrap().push(json!({"code":"DEVUP_CODEGEN_INTERACTION_UNVERIFIED","severity":"warning","fidelityImpact":"none","message":"Interaction/state fields could not be read; candidate coverage is incomplete.","details":{"classification":"interaction-unverified","nextAction":"Review interactionReadErrors and recollect failed fields before claiming interaction coverage."}}));
        if result["deliverable"].is_object() {
            result["deliverable"]["isFinal"] = json!(false);
            result["deliverable"]["note"] =
                json!("Interaction evidence is incomplete; review failed source fields.");
        }
    }
    if count > 0 {
        let issues = result
            .as_object_mut()
            .unwrap()
            .entry("projectionIssues")
            .or_insert(json!([]));
        let owners = devup_mcp_figma::screen_owners(snapshot, roots.iter().map(String::as_str));
        let mut groups = BTreeMap::<Option<String>, Vec<Value>>::new();
        for candidate in result_candidates(&data) {
            let id = candidate["nodeId"].as_str().unwrap();
            groups
                .entry(owners.get(id).cloned().flatten())
                .or_default()
                .push(candidate["nodeId"].clone());
        }
        for (screen_id, node_ids) in groups {
            issues.as_array_mut().unwrap().push(json!({"code":"DEVUP_CODEGEN_INTERACTION_UNMAPPED","nodeId":node_ids.first(),"screenId":screen_id,
                "property":"reactions","severity":"warning","fidelityImpact":"none",
                "message":"Interaction candidate has static markup only; href/events and accessible control semantics require implementation.",
                "details":{"classification":"interaction-unmapped","candidateCount":node_ids.len(),"nodeIds":node_ids,"nextAction":"Resolve every interactiveCandidate against real routes, record IDs and accessible controls; validate and test the wiring. Ask about unresolved targets together; never invent a destination."}}));
        }
        if result["deliverable"].is_object() {
            result["deliverable"]["isFinal"] = json!(false);
            result["deliverable"]["note"] = json!(
                "Static visual output is available; interaction candidates require wiring and verification before functional completion."
            );
        }
    }
    let mut questions = vec![];
    let unknown_targets = result_candidates(&data)
        .iter()
        .filter(|c| {
            c["prototypeActions"]
                .as_array()
                .is_none_or(|a| a.is_empty() || a.iter().any(|a| !a["url"].is_string()))
        })
        .map(|c| c["nodeId"].clone())
        .collect::<Vec<_>>();
    if !unknown_targets.is_empty() {
        questions.push(json!({"topic":"destinations-and-actions","nodeIds":unknown_targets,"question":"Confirm unresolved destinations, record IDs and local actions together; which candidates intentionally have no behavior?"}));
    }
    let logos = result_candidates(&data)
        .iter()
        .filter(|c| {
            c["name"]
                .as_str()
                .is_some_and(|s| has(s, &["logo", "로고"]))
        })
        .collect::<Vec<_>>();
    if logos
        .iter()
        .filter_map(|c| c["name"].as_str())
        .collect::<BTreeSet<_>>()
        .len()
        > 1
    {
        questions.push(json!({"topic":"logo-versions","nodeIds":logos.iter().map(|c|c["nodeId"].clone()).collect::<Vec<_>>(),"question":"Different logo layer names were captured; confirm the intended version per placement before choosing or reusing assets."}));
    }
    if data["sharedComponentCandidates"]
        .as_array()
        .is_some_and(|v| v.iter().any(|v| v["variantDifferences"] == true))
    {
        questions.push(json!({"topic":"shared-component-variants","question":"Which menu/label/state version applies to each screen, and should any version become site-wide?"}));
    }
    if data["widgetCandidates"]
        .as_array()
        .is_some_and(|v| !v.is_empty())
    {
        questions.push(json!({"topic":"third-party-widgets","question":"Confirm widget provider and existing integration/env key names without sharing values."}));
    }
    result["ambiguityQuestions"] = json!(questions);
    result["completionEvidence"] = json!({"behavior":"unverified","visual":"unverified","required":["Report candidate, wired and tested counts with unresolved reasons.","Run devup_ui_validate with real route inventory and inspect source/rendered href coverage.","Render at each implemented viewport with project fonts/assets loaded and capture a screenshot; call devup_visual_compare against referencePng. Report skipped comparisons and remaining viewports explicitly."],
        "visualCompare":{"tool":"devup_visual_compare","automaticRun":false,"reason":"Requires actual browser-rendered screenshot and matching Figma reference; source analysis is not pixel evidence."}});
    if let Some(frames) = result.get_mut("frames").and_then(Value::as_array_mut) {
        for frame in frames {
            let root = frame["nodeId"].as_str().unwrap_or("").to_owned();
            let data = evidence(snapshot, &[root]);
            append_hints(frame, &data);
            frame["interactionCandidateIds"] = json!(
                result_candidates(&data)
                    .iter()
                    .map(|c| c["nodeId"].clone())
                    .collect::<Vec<_>>()
            );
            frame["interactionEvidencePath"] = json!("/interactiveCandidates");
            frame["interactionCoverage"] = data["interactionCoverage"].clone();
            if data["interactiveCandidates"]
                .as_array()
                .is_some_and(|c| !c.is_empty())
                && frame["deliverable"].is_object()
            {
                frame["deliverable"]["isFinal"] = json!(false);
                frame["deliverable"]["note"] = json!(
                    "Static visual output; interactions need wiring and runtime verification."
                );
            }
        }
    }
}

fn result_candidates(value: &Value) -> &[Value] {
    value["interactiveCandidates"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
}
