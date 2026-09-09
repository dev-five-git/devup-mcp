use std::collections::{BTreeMap, BTreeSet};

use devup_mcp_devup_ui::codegen::CodegenOutput;
use devup_mcp_figma::{Diagnostic, FidelityImpact};
use serde_json::{Map, Value, json};

pub(super) fn scope_output(output: &mut CodegenOutput, field: &str) {
    for diagnostic in &mut output.diagnostics {
        let details = diagnostic.details.get_or_insert_with(|| json!({}));
        details["output"] = json!(field);
    }
}

pub(super) fn output_result(output: &CodegenOutput) -> Value {
    let quality = super::quality::projection_quality(true, &output.diagnostics);
    json!({"state":"produced","projection":quality,"fidelity":output.fidelity_report})
}

pub(super) fn failure_issue(failure: &Value) -> Diagnostic {
    Diagnostic {
        code: failure["errorCode"]
            .as_str()
            .unwrap_or("DEVUP_CODEGEN_FAILED")
            .into(),
        node_id: failure["nodeId"].as_str().map(str::to_owned),
        property: Some(
            if failure.get("assetId").is_some() {
                "assetReference"
            } else {
                "output"
            }
            .into(),
        ),
        message: failure["message"]
            .as_str()
            .unwrap_or("Requested output could not be delivered.")
            .into(),
        fidelity_impact: Some(FidelityImpact::Lossy),
        details: Some(json!({"output":failure["output"],"stage":"projection",
        "originalValue":{"requestedOutput":failure["output"],"assetId":failure["assetId"],"failureDetails":failure["details"]},
        "appliedValue":{"state":"withheld","output":failure["output"]},
        "classification":"withheld-output",
        "nextAction":if failure.get("assetId").is_some() {
            "Review the excluded asset and its visibility/clipping; correct the source or report the conflicting asset ID and output. Successful outputs remain usable."
        } else {
            "Read the failure details and resume guidance. Collect missing source/component nodes or request inline tsx; successful outputs remain usable."
        }})),
        ..Diagnostic::default()
    }
}

pub(super) fn attach_review_and_deliverable(
    result: &mut Map<String, Value>,
    requested: &[String],
    failures: &[Value],
    node_id: Option<&str>,
) {
    let issues = result
        .get("projectionIssues")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut groups = BTreeMap::<(u8, String, String, String), Vec<usize>>::new();
    for (index, issue) in issues.iter().enumerate() {
        let details = &issue["details"];
        let class = details["classification"]
            .as_str()
            .unwrap_or("approximation");
        let priority = match class {
            "withheld-output" => 1,
            "unclassified" | "property-unmapped" => 2,
            "component-reference" => 3,
            _ => 4,
        };
        let owner = details["appliedValue"]["generatedNodeId"]
            .as_str()
            .or(issue["nodeId"].as_str())
            .unwrap_or("");
        groups
            .entry((
                priority,
                details["output"].as_str().unwrap_or("unknown").into(),
                owner.into(),
                class.into(),
            ))
            .or_default()
            .push(index);
    }
    let mut groups = groups.into_iter().map(|((priority, output, owner, class), indexes)| {
        let nodes: BTreeSet<_> = indexes.iter().filter_map(|&i| issues[i]["nodeId"].as_str()).collect();
        let properties: BTreeSet<_> = indexes.iter().filter_map(|&i| issues[i]["property"].as_str()).collect();
        json!({"priority":priority,"output":output,"representativeNodeId":owner,"classification":class,
            "issueCount":indexes.len(),"affectedNodeCount":nodes.len(),"affectedNodeIds":nodes,"properties":properties,
            "nextAction":issues[indexes[0]]["details"]["nextAction"].as_str().unwrap_or("Compare the generated evidence with the original property before accepting this approximation."),"issueIndexes":indexes})
    }).collect::<Vec<_>>();
    groups.sort_by(|a, b| {
        a["priority"]
            .as_u64()
            .cmp(&b["priority"].as_u64())
            .then_with(|| b["issueCount"].as_u64().cmp(&a["issueCount"].as_u64()))
            .then_with(|| a["output"].as_str().cmp(&b["output"].as_str()))
            .then_with(|| {
                a["representativeNodeId"]
                    .as_str()
                    .cmp(&b["representativeNodeId"].as_str())
            })
    });
    if !issues.is_empty() {
        result.insert("projectionReview".into(), json!({"issueCount":issues.len(),"groups":groups,
            "order":"Withheld outputs first, then unmapped properties, component references, and other approximations; larger groups first within each priority. Priority is review order, not measured pixel impact."}));
    }
    if !requested
        .iter()
        .any(|s| matches!(s.as_str(), "tsx" | "componentTsx" | "responsiveTsx"))
    {
        return;
    }
    let mut available = Vec::new();
    for field in ["tsx", "componentTsx", "responsiveTsx"] {
        if result.get(field).is_some_and(Value::is_string) {
            available.push(json!({"nodeId":node_id,"output":field}));
        }
    }
    if let Some(frames) = result.get("frames").and_then(Value::as_array) {
        for frame in frames {
            for field in ["tsx", "componentTsx"] {
                if frame.get(field).is_some_and(Value::is_string) {
                    available.push(json!({"nodeId":frame["nodeId"],"output":field}));
                }
            }
        }
    }
    let withheld: Vec<_> = failures
        .iter()
        .filter(|f| f.get("output").is_some())
        .map(|f| json!({"nodeId":f["nodeId"],"output":f["output"],"errorCode":f["errorCode"]}))
        .collect();
    for failure in &withheld {
        let field = failure["output"].as_str().unwrap();
        if !result.contains_key("frames") {
            let outputs = result.entry("outputResults").or_insert_with(|| json!({}));
            outputs[field] =
                json!({"state":"withheld","projection":"failed","errorCode":failure["errorCode"]});
        }
    }
    let is_final = result.get("status").and_then(Value::as_str) == Some("complete")
        && !available.is_empty()
        && withheld.is_empty();
    result.insert("deliverable".into(), json!({"kind":"devup-ui-tsx","isFinal":is_final,
        "availableOutputs":available,"withheldOutputs":withheld,
        "note":if is_final {"This tsx is the final deliverable. Implement from this value."} else {"Review projectionIssues and outputResults before using available outputs. Withheld outputs have no code; failures supplies their operational context."}}));
}
