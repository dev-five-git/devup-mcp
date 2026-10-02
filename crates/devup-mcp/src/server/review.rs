//! Summary-first digest of an export response.
//!
//! The full response stays as it is; this adds a short `summary` and, when the
//! result is not exact/complete, a `reviewChecklist` that says what a human
//! still has to check, where, and how. Everything is derived from fields the
//! response already carries, so nothing here can disagree with them.
use serde_json::{Value, json};

const MAX_ITEMS: usize = 20;

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}

/// One checklist item per diagnostic that needs a human, grouped by code so a
/// hundred identical findings read as one line with a node list.
fn items_from_issues(scope: &str, issues: &[Value], out: &mut Vec<Value>) {
    let mut groups: Vec<(String, Vec<&Value>)> = vec![];
    for issue in issues {
        let code = text(&issue["code"]).to_owned();
        match groups.iter_mut().find(|(c, _)| *c == code) {
            Some((_, members)) => members.push(issue),
            None => groups.push((code, vec![issue])),
        }
    }
    for (code, members) in groups {
        let lossy = members
            .iter()
            .any(|m| matches!(text(&m["fidelityImpact"]), "lossy" | "failed"));
        let nodes: Vec<&Value> = members.iter().map(|m| &m["nodeId"]).collect();
        let first = members[0];
        let how = first["details"]["nextAction"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| first["details"]["nextAction"]["how"].as_str().map(str::to_owned))
            .unwrap_or_else(|| {
                "Compare the generated property with the raw Figma value for these nodes (export rawSnapshot with debug: true) before shipping.".into()
            });
        out.push(json!({
            "severity": if lossy { "required" } else { "recommended" },
            "scope": scope,
            "code": code,
            "count": members.len(),
            "what": first["message"],
            "properties": members.iter().filter_map(|m| m["property"].as_str()).collect::<std::collections::BTreeSet<_>>(),
            "nodeIds": nodes.into_iter().take(10).collect::<Vec<_>>(),
            "how": how,
        }));
    }
}

/// `round(fontSize * percent / 100)` is the one unit conversion the generator
/// performs that a reader cannot reproduce from the raw field alone: Figma
/// stores `lineHeight: {unit: PERCENT, value: 160}`, the TSX says `26px`.
fn has_line_height(value: &Value) -> bool {
    value["sourceMap"]["entries"]
        .as_array()
        .is_some_and(|entries| {
            entries
                .iter()
                .any(|e| matches!(text(&e["property"]), "lineHeight" | "styledTextSegments"))
        })
}

fn collect(response: &Value, scope: &str, out: &mut Vec<Value>) {
    if let Some(issues) = response["projectionIssues"].as_array() {
        items_from_issues(scope, issues, out);
    }
    if response["quality"]["assets"] == "partial" || response["quality"]["assets"] == "failed" {
        out.push(json!({"severity":"required","scope":scope,"code":"ASSETS_INCOMPLETE",
            "what":"Some assets were not collected.","how":"Read assetSummary.unavailable[]; re-request those assets by id with assetRequests (original url, not artifactId)."}));
    }
    if response["quality"]["acquisition"] == "partial" {
        out.push(json!({"severity":"recommended","scope":scope,"code":"ACQUISITION_PARTIAL",
            "what":"The Figma snapshot was incomplete.","how":"Check completenessReport for what is missing; re-export with refresh:true if the missing part matters."}));
    }
    if has_line_height(response) {
        out.push(json!({"severity":"recommended","scope":scope,"code":"LINE_HEIGHT_CONVERSION",
            "what":"Percent line-height is written as px.",
            "how":"Verify with round(fontSize * percent / 100): e.g. 160% at 16.25px = 26px. When fontSize is bound to a variable the generator keeps the percentage (`160%`) instead of px."}));
    }
}

/// Add `summary` and, only when something needs review, `reviewChecklist`.
pub(super) fn attach(result: &mut Value) {
    let mut items = vec![];
    collect(result, "response", &mut items);
    if let Some(frames) = result["frames"].as_array() {
        for frame in frames {
            let scope = frame["nodeId"].as_str().unwrap_or("frame");
            collect(frame, scope, &mut items);
        }
    }
    let required = items.iter().filter(|i| i["severity"] == "required").count();
    let recommended = items.len() - required;
    let projection = result["quality"]["projection"].clone();
    let next = if required > 0 {
        "Resolve every `required` item in reviewChecklist before using this output."
    } else if recommended > 0 {
        "Usable; spot-check the `recommended` items in reviewChecklist."
    } else {
        "Nothing to review."
    };
    if let Some(object) = result.as_object_mut() {
        object.insert(
            "summary".into(),
            json!({"status":object.get("status"),"projection":projection,
                "required":required,"recommended":recommended,"next":next}),
        );
        if !items.is_empty() {
            items.sort_by_key(|i| i["severity"] != "required");
            items.truncate(MAX_ITEMS);
            object.insert("reviewChecklist".into(), json!(items));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_response_has_summary_and_no_checklist() {
        let mut value =
            json!({"status":"complete","quality":{"projection":"exact","assets":"not-requested"}});
        attach(&mut value);
        assert_eq!(value["summary"]["required"], 0);
        assert!(value.get("reviewChecklist").is_none());
    }

    #[test]
    fn issues_group_by_code_and_lossy_is_required() {
        let mut value = json!({"status":"partial","quality":{"projection":"lossy"},
            "projectionIssues":[
                {"code":"A","nodeId":"1:1","property":"w","fidelityImpact":"lossy","message":"m"},
                {"code":"A","nodeId":"1:2","property":"h","fidelityImpact":"lossy","message":"m"},
                {"code":"DEVUP_CODEGEN_PROPERTY_UNMAPPED","nodeId":"1:3","property":"x","fidelityImpact":"none","message":"u"}]});
        attach(&mut value);
        let list = value["reviewChecklist"].as_array().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0]["severity"], "required");
        assert_eq!(list[0]["count"], 2);
        assert_eq!(value["summary"]["required"], 1);
        assert_eq!(value["summary"]["recommended"], 1);
    }

    #[test]
    fn line_height_conversion_is_explained_when_typography_is_mapped() {
        let mut value = json!({"status":"complete","quality":{"projection":"exact"},
            "sourceMap":{"entries":[{"property":"lineHeight"}]}});
        attach(&mut value);
        assert_eq!(
            value["reviewChecklist"][0]["code"],
            "LINE_HEIGHT_CONVERSION"
        );
    }
}
