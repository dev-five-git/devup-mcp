//! Bounded data selection, never an executable query language.
use devup_mcp_figma::{DevupError, ErrorCode, Snapshot};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeSet, VecDeque};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResponseSelection {
    /// At most 16 paths: dot fields, [index], [name="exact name"] or [id="exact id"].
    /// nodes selects captured node summaries, e.g. nodes[name="Cards"].layout.
    #[serde(default)]
    #[schemars(extend("maxItems" = 16))]
    pub response_fields: Vec<String>,
    #[serde(default)]
    pub path: Option<String>,
    /// Stable captured-order offset for outline and each selected path.
    #[serde(default)]
    pub response_offset: usize,
    #[serde(default = "default_limit")]
    #[schemars(range(min = 1, max = 200))]
    pub response_limit: usize,
}
fn default_limit() -> usize {
    100
}
impl Default for ResponseSelection {
    fn default() -> Self {
        Self {
            response_fields: vec![],
            path: None,
            response_offset: 0,
            response_limit: default_limit(),
        }
    }
}

#[derive(Debug, PartialEq)]
enum Step {
    Field(String),
    Index(usize),
    Exact(String, String),
}
fn invalid() -> DevupError {
    DevupError::new(
        ErrorCode::DevupInvalidInput,
        "Selection accepts at most 16 paths of 256 bytes and 8 steps: dot fields, [index], [name=\"exact\"] or [id=\"exact\"]. No wildcards, code, recursion or expressions. responseLimit is 1..200; responseOffset is at most 100000.",
        false,
    )
}
fn parse(path: &str) -> Result<Vec<Step>, DevupError> {
    if path.is_empty() || path.len() > 256 {
        return Err(invalid());
    }
    let mut steps = Vec::new();
    let mut rest = path;
    loop {
        let end = rest.find(['.', '[']).unwrap_or(rest.len());
        let key = &rest[..end];
        if key.is_empty()
            || !key
                .bytes()
                .enumerate()
                .all(|(i, b)| b.is_ascii_alphabetic() || b == b'_' || (i > 0 && b.is_ascii_digit()))
            || matches!(key, "__proto__" | "constructor" | "prototype")
        {
            return Err(invalid());
        }
        steps.push(Step::Field(key.into()));
        rest = &rest[end..];
        while let Some(after) = rest.strip_prefix('[') {
            if after.starts_with("name=") || after.starts_with("id=") {
                let (field, value) = after.split_once('=').ok_or_else(invalid)?;
                let mut decoded = serde_json::Deserializer::from_str(value).into_iter::<String>();
                let value_decoded = decoded.next().ok_or_else(invalid)?.map_err(|_| invalid())?;
                let tail = &value[decoded.byte_offset()..];
                rest = tail.strip_prefix(']').ok_or_else(invalid)?;
                steps.push(Step::Exact(field.into(), value_decoded));
            } else {
                let (index, tail) = after.split_once(']').ok_or_else(invalid)?;
                if index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(invalid());
                }
                let index = index.parse::<usize>().map_err(|_| invalid())?;
                if index > 100_000 {
                    return Err(invalid());
                }
                steps.push(Step::Index(index));
                rest = tail;
            }
        }
        if steps.len() > 8 {
            return Err(invalid());
        }
        if rest.is_empty() {
            break;
        }
        rest = rest.strip_prefix('.').ok_or_else(invalid)?;
    }
    Ok(steps)
}
impl ResponseSelection {
    pub fn validate(&self) -> Result<(), DevupError> {
        if self.response_fields.len() + usize::from(self.path.is_some()) > 16
            || self.response_limit == 0
            || self.response_limit > 200
            || self.response_offset > 100_000
        {
            return Err(invalid());
        }
        for path in self.response_fields.iter().chain(self.path.iter()) {
            parse(path)?;
        }
        Ok(())
    }
    pub fn active(&self) -> bool {
        !self.response_fields.is_empty() || self.path.is_some()
    }
}

pub fn layout(node: &devup_mcp_figma::RawNode) -> Value {
    let number = |key| {
        node.fields
            .get(key)
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite())
            .map_or(Value::Null, |n| json!(n))
    };
    let label = |key| {
        node.fields
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| s.len() <= 32)
    };
    json!({"width":number("width"),"height":number("height"),
        "padding":{"top":number("paddingTop"),"right":number("paddingRight"),"bottom":number("paddingBottom"),"left":number("paddingLeft")},
        "gap":number("itemSpacing"),"layoutMode":label("layoutMode"),
        "align":{"primary":label("primaryAxisAlignItems"),"counter":label("counterAxisAlignItems")}})
}

pub fn node_summaries(snapshot: &Snapshot, roots: &[String]) -> Value {
    node_summaries_bounded(snapshot, roots, 100_000)
}

fn node_summaries_bounded(snapshot: &Snapshot, roots: &[String], scan_limit: usize) -> Value {
    let mut queue = roots
        .iter()
        .map(|id| (id.as_str(), 0))
        .collect::<VecDeque<_>>();
    let mut seen = BTreeSet::new();
    let mut nodes = Vec::new();
    let mut missing = Vec::new();
    while let Some((id, depth)) = queue.pop_front() {
        if !seen.insert(id) {
            continue;
        }
        if seen.len() > scan_limit {
            break;
        }
        let Some(node) = snapshot.nodes.get(id) else {
            missing.push(id);
            continue;
        };
        let view = node.typed_view();
        let children = view.child_ids().collect::<Vec<_>>();
        queue.extend(children.iter().map(|id| (*id, depth + 1)));
        nodes.push(json!({"id":id,"nodeId":id,"name":view.name().map(|s| s.chars().take(256).collect::<String>()),
            "type":node.node_type,"parentId":view.string("parentId"),"depth":depth,"childCount":children.len(),"layout":layout(node)}));
    }
    json!({"nodes":nodes,"scope":"captured selected roots and descendants","roots":roots,
        "scanTruncated":seen.len()>scan_limit || !queue.is_empty(),"missingCapturedNodes":missing.len(),
        "evidenceLimit":"Only captured fields are summarized; null is unavailable, never zero. No live selection or rendered implementation is verified."})
}

pub fn outline(summary: &Value, options: &ResponseSelection) -> Value {
    let nodes = summary["nodes"].as_array().expect("node summaries");
    let end = nodes.len().min(
        options
            .response_offset
            .saturating_add(options.response_limit),
    );
    let page = nodes
        .iter()
        .skip(options.response_offset)
        .take(options.response_limit)
        .cloned()
        .collect::<Vec<_>>();
    json!({"nodes":page,"coordinateSpace":"figma-logical","scope":summary["scope"],"roots":summary["roots"],
        "pagination":{"offset":options.response_offset,"returned":page.len(),"totalCaptured":nodes.len(),
            "nextOffset":(end < nodes.len()).then_some(end),"complete":end >= nodes.len() && summary["scanTruncated"] == false && summary["missingCapturedNodes"] == 0},
        "scanTruncated":summary["scanTruncated"],"missingCapturedNodes":summary["missingCapturedNodes"],"evidenceLimit":summary["evidenceLimit"]})
}

pub fn select(
    result: &mut serde_json::Map<String, Value>,
    summary: &Value,
    options: &ResponseSelection,
) -> Result<Option<super::delivery::ProjectedOutput>, DevupError> {
    if !options.active() {
        return Ok(None);
    }
    options.validate()?;
    let mut root = Value::Object(result.clone());
    root["nodes"] = summary["nodes"].clone();
    let mut selections = Vec::new();
    for path in options.response_fields.iter().chain(options.path.iter()) {
        let mut values = vec![(&root, None::<String>)];
        for step in parse(path)? {
            values = match step {
                Step::Field(key) => values
                    .into_iter()
                    .flat_map(|(v, id)| {
                        if let Some(items) = v.as_array() {
                            items
                                .iter()
                                .filter_map(|v| {
                                    v.get(&key).map(|field| {
                                        (
                                            field,
                                            v.get("nodeId")
                                                .or_else(|| v.get("id"))
                                                .and_then(Value::as_str)
                                                .map(str::to_owned)
                                                .or_else(|| id.clone()),
                                        )
                                    })
                                })
                                .collect::<Vec<_>>()
                        } else {
                            v.get(&key).map(|field| (field, id)).into_iter().collect()
                        }
                    })
                    .collect(),
                Step::Index(index) => values
                    .into_iter()
                    .filter_map(|(v, id)| {
                        v.get(index).map(|item| {
                            (
                                item,
                                item.get("nodeId")
                                    .or_else(|| item.get("id"))
                                    .and_then(Value::as_str)
                                    .map(str::to_owned)
                                    .or(id),
                            )
                        })
                    })
                    .collect(),
                Step::Exact(key, value) => values
                    .into_iter()
                    .flat_map(|(v, _)| v.as_array().into_iter().flatten())
                    .filter(|v| v.get(&key).and_then(Value::as_str) == Some(value.as_str()))
                    .map(|v| {
                        (
                            v,
                            v.get("nodeId")
                                .or_else(|| v.get("id"))
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                        )
                    })
                    .collect(),
            };
        }
        let count = values.len();
        let end = count.min(
            options
                .response_offset
                .saturating_add(options.response_limit),
        );
        let identities = values
            .iter()
            .skip(options.response_offset)
            .take(options.response_limit)
            .map(|(_, id)| json!(id))
            .collect::<Vec<_>>();
        let values = values
            .into_iter()
            .skip(options.response_offset)
            .take(options.response_limit)
            .map(|(v, _)| v.clone())
            .collect::<Vec<_>>();
        selections.push(json!({"path":path,"values":values,"matchedCaptured":count,
            "nodeIds":identities,
            "offset":options.response_offset,"nextOffset":(end < count).then_some(end),
            "state":if count == 0 {"not-found-in-capture"} else {"selected"},"truncated":end < count}));
    }
    let selected = json!({"selections":selections,"coordinateSpace":"figma-logical",
        "scanTruncated":summary["scanTruncated"],"missingCapturedNodes":summary["missingCapturedNodes"],
        "evidenceLimit":summary["evidenceLimit"]});
    if serde_json::to_vec(&selected).unwrap_or_default().len() > 65_536 {
        return Err(DevupError::new(
            ErrorCode::DevupFigmaResponseTooLarge,
            "Selected data exceeds 65536 bytes. Select a narrower field or lower responseLimit; a scalar is never silently truncated.",
            false,
        ));
    }
    let evidence_fields = [
        "projectionIssues",
        "projectionEvidence",
        "completenessReport",
        "fidelity",
        "verdictScope",
        "outputResults",
        "failures",
        "warnings",
        "responsiveReviewChecklist",
        "sectionChecklist",
        "projectionReview",
        "placementContracts",
        "widthBehavior",
        "imageFillAudit",
        "imageAudit",
        "assetCssMap",
        "assetSummary",
        "widgetCandidates",
        "sharedComponentCandidates",
        "breakpointCompletion",
    ];
    let mut details = serde_json::Map::new();
    for field in evidence_fields {
        if let Some(value) = result.get(field) {
            details.insert(field.into(), value.clone());
        }
    }
    if let Some(frames) = result.get("frames").and_then(Value::as_array) {
        details.insert("frames".into(),json!(frames.iter().map(|f|json!({"nodeId":f["nodeId"],"failures":f["failures"],"outputResults":f["outputResults"],"projectionIssues":f["projectionIssues"],"fidelity":f["fidelity"]})).collect::<Vec<_>>()));
    }
    let issue_count = result
        .get("projectionIssues")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let issues=result.get("projectionIssues").and_then(Value::as_array).map(|items|items.iter().take(10).map(|v|json!({"code":v["code"],"nodeId":v["nodeId"],"property":v["property"],"fidelityImpact":v["fidelityImpact"],"message":v["message"].as_str().map(|s|s.chars().take(240).collect::<String>())})).collect::<Vec<_>>()).unwrap_or_default();
    let output_results = result.get("outputResults").and_then(Value::as_object).map(|items| items.iter().map(|(name,v)|(name.clone(),json!({"state":v["state"],"projection":v["projection"],"bodyDelivery":"omitted-by-selection"}))).collect::<serde_json::Map<_,_>>());
    result.retain(|key, _| {
        matches!(
            key.as_str(),
            "status"
                | "source"
                | "cache"
                | "quality"
                | "completeness"
                | "collection"
                | "verificationScope"
                | "outputPaths"
                | "outputPathResults"
                | "resume"
                | "responsiveEvidence"
                | "mappingComplete"
        )
    });
    result.insert("projectionIssues".into(), json!(issues));
    result.insert("selectionEvidence".into(),json!({"name":"compact-details.json","fields":evidence_fields,
        "projectionIssueCount":issue_count,"returnedProjectionIssues":issues.len(),"issuesTruncated":issue_count>issues.len(),
        "requiresFullDetailRead":true,"scope":"Complete omitted evidence for the original requested projection; selected-data delivery does not establish code completeness."}));
    result.insert("deliverable".into(),json!({"kind":"selected-data","state":"data-only","codeDelivered":false,
        "evidenceLimit":"Quality describes source acquisition and requested projections; selected fields do not deliver or verify a complete application."}));
    if let Some(outputs) = output_results {
        result.insert("outputResults".into(), json!(outputs));
    }
    result.insert("selected".into(), selected);
    Ok(Some(super::delivery::ProjectedOutput::text(
        "compact-details.json",
        "application/json",
        serde_json::to_vec(&details).map_err(|_| invalid())?,
    )))
}

/// Standalone outlines keep the small data body inline and move unrelated
/// full acquisition/projection evidence into the same bounded resource path.
pub fn outline_response(
    result: &mut serde_json::Map<String, Value>,
    summary: &Value,
) -> Result<Option<super::delivery::ProjectedOutput>, DevupError> {
    let body = result.get("outline").cloned().unwrap_or(Value::Null);
    let details = select(
        result,
        summary,
        &ResponseSelection {
            path: Some("outline".into()),
            response_limit: 1,
            ..Default::default()
        },
    )?;
    result.remove("selected");
    result.insert("outline".into(), body);
    result.get_mut("deliverable").unwrap()["kind"] = json!("outline-data");
    Ok(details)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_last_node_past_the_scan_cap_still_marks_partial_outline_data() {
        let snapshot: Snapshot =
            serde_json::from_value(json!({"fileKey":"Synthetic","roots":["1:1"],"nodes":{
            "1:1":{"id":"1:1","type":"FRAME","fields":{"childrenIds":["1:2"]}},
            "1:2":{"id":"1:2","type":"FRAME","fields":{"childrenIds":["1:3"]}},
            "1:3":{"id":"1:3","type":"TEXT","fields":{}}
        },"diagnostics":[]}))
            .unwrap();
        let summary = node_summaries_bounded(&snapshot, &snapshot.roots, 2);
        assert_eq!(summary["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(summary["scanTruncated"], true);
        assert_eq!(
            outline(&summary, &ResponseSelection::default())["pagination"]["complete"],
            false
        );
        let complete = node_summaries_bounded(&snapshot, &snapshot.roots, 3);
        assert_eq!(complete["scanTruncated"], false);
        assert_eq!(
            outline(&complete, &ResponseSelection::default())["pagination"]["complete"],
            true
        );
    }
    #[test]
    fn grammar_refuses_execution_recursion_and_overruns() {
        for path in [
            "nodes[*]",
            "nodes.filter(x)",
            "nodes..layout",
            "nodes[0];alert(1)",
            "__proto__.x",
            "nodes[name='x']",
            "nodes[100001]",
            "nodes[name=\"x\"]tail",
        ] {
            assert!(parse(path).is_err(), "{path}");
        }
        assert!(parse("nodes[name=\"Cards\"].layout.padding").is_ok());
        assert!(parse("frames[0].tsx").is_ok());
    }
    #[test]
    fn exact_unicode_selection_and_pagination_keep_identity() {
        let summary = json!({"nodes":[{"id":"1:1","name":"Cards","layout":{"width":167}},{"id":"1:2","name":"Cards","layout":{"width":195}}],"scanTruncated":false,"missingCapturedNodes":0});
        let mut result =
            json!({"tsx":"huge code","cache":{"artifactId":"stable"},"failures":[{"code":"gap"}]})
                .as_object()
                .unwrap()
                .clone();
        let options = ResponseSelection {
            path: Some("nodes[name=\"Cards\"].layout".into()),
            response_limit: 1,
            ..Default::default()
        };
        select(&mut result, &summary, &options).unwrap();
        assert_eq!(
            result["selected"]["selections"][0]["values"][0]["width"],
            167
        );
        assert_eq!(result["selected"]["selections"][0]["nextOffset"], 1);
        assert_eq!(
            result["selected"]["selections"][0]["nodeIds"],
            json!(["1:1"])
        );
        assert_eq!(result["cache"]["artifactId"], "stable");
        assert!(result.get("tsx").is_none());
        assert_eq!(outline(&summary, &options)["pagination"]["complete"], false);
    }
    #[test]
    fn bulky_evidence_has_a_full_resource_and_bounded_inline_preview() {
        let mut result=json!({"status":"partial","quality":{"projection":"lossy"},"projectionIssues":(0..500).map(|i|json!({"code":"SYNTHETIC_GAP","nodeId":format!("1:{i}"),"message":"x".repeat(1000),"details":{"value":"y".repeat(1000)}})).collect::<Vec<_>>(),"tsx":"large code","outputResults":{"tsx":{"state":"produced","projection":"lossy"}}}).as_object().unwrap().clone();
        let summary = json!({"nodes":[{"id":"1:1","name":"Cards","layout":{"width":167}}],"scanTruncated":false,"missingCapturedNodes":0});
        let options = ResponseSelection {
            path: Some("nodes[id=\"1:1\"].layout".into()),
            ..Default::default()
        };
        let resource = select(&mut result, &summary, &options).unwrap().unwrap();
        assert!(resource.bytes.len() > 1_000_000);
        assert!(serde_json::to_vec(&result).unwrap().len() < 10_000);
        assert_eq!(result["status"], "partial");
        assert_eq!(result["selectionEvidence"]["projectionIssueCount"], 500);
        assert_eq!(result["projectionIssues"].as_array().unwrap().len(), 10);
        assert_eq!(result["deliverable"]["codeDelivered"], false);
        assert_eq!(
            result["outputResults"]["tsx"]["bodyDelivery"],
            "omitted-by-selection"
        );
    }
    #[test]
    fn standalone_outline_keeps_only_bounded_data_and_links_verbose_evidence() {
        let body = json!({"nodes":[{"id":"1:1","layout":{"width":167}}],"pagination":{"offset":2,"nextOffset":3}});
        let mut result=json!({"status":"partial","outline":body,"projectionIssues":(0..500).map(|_|json!({"code":"SYNTHETIC_GAP","message":"x".repeat(1000),"details":{"raw":"y".repeat(1000)}})).collect::<Vec<_>>(),"imageFillAudit":{"unrelated":"z".repeat(50000)}}).as_object().unwrap().clone();
        let details = outline_response(
            &mut result,
            &json!({"nodes":[],"scanTruncated":false,"missingCapturedNodes":0}),
        )
        .unwrap()
        .unwrap();
        assert_eq!(result["outline"], body);
        assert_eq!(result["status"], "partial");
        assert_eq!(result["deliverable"]["kind"], "outline-data");
        assert!(result.get("selected").is_none());
        assert!(result.get("imageFillAudit").is_none());
        assert!(serde_json::to_vec(&result).unwrap().len() < 10000);
        assert!(details.bytes.len() > 1_000_000);
    }
}
