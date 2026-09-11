//! Explain the final verdict without introducing another grading policy.
use serde_json::{Value, json};

pub(super) fn attach(value: &mut Value) {
    if let Some(frames) = value.get_mut("frames").and_then(Value::as_array_mut) {
        for frame in frames {
            attach(frame);
        }
    }
    let mut projection_causes = Vec::new();
    for (index, issue) in value["projectionIssues"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let details = &issue["details"];
        let cause = json!({"diagnosticIndex":index,
            "screenId":issue["screenId"],"nodeId":issue["nodeId"],
            "output":details["output"],"code":issue["code"],"property":issue["property"],
            "fidelityImpact":issue["fidelityImpact"]});
        let unresolved: Vec<_> = ["height", "width", "horizontal", "vertical"]
            .into_iter()
            .filter(|axis| {
                details["components"][*axis].is_object()
                    && details["components"][*axis]["fidelityImpact"] != "none"
            })
            .collect();
        if unresolved.is_empty() {
            let mut cause = cause;
            cause["reason"] = issue["message"].clone();
            projection_causes.push(cause);
        } else {
            for axis in unresolved {
                let component = &details["components"][axis];
                let mut cause = cause.clone();
                cause["component"] = json!(axis);
                for key in ["state", "reason", "blockedBy", "resolutionCondition"] {
                    if let Some(field) = component.get(key) {
                        cause[key] = field.clone();
                    }
                }
                projection_causes.push(cause);
            }
        }
    }
    let mut status_causes = Vec::new();
    for (domain, accepted, evidence) in [
        (
            "acquisition",
            &["complete", "expected-projection"][..],
            "completenessReport",
        ),
        (
            "projection",
            &["exact", "not-requested"][..],
            "verdictScope.projectionCauses",
        ),
        (
            "theme",
            &["complete", "not-requested"][..],
            "themeCompleteness",
        ),
        ("assets", &["complete", "not-requested"][..], "assetSummary"),
    ] {
        if let Some(state) = value["quality"][domain].as_str()
            && !accepted.contains(&state)
        {
            status_causes.push(json!({"domain":domain,"state":state,"evidence":evidence,
                "evidenceScope":if domain == "projection" { "self" } else { "response" }}));
        }
    }
    for (index, failure) in value["failures"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        status_causes.push(json!({"domain":"requested-output","failureIndex":index,
            "nodeId":failure["nodeId"],"output":failure["output"],
            "code":failure["errorCode"],"reason":failure["message"]}));
    }
    value["verdictScope"] = json!({
        "status":value["status"],"projection":value["quality"]["projection"],
        "statusCauses":status_causes,"projectionCauses":projection_causes
    });
}
