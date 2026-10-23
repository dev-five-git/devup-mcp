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
        let nodes: Vec<&Value> = members
            .iter()
            .flat_map(|m| {
                m["details"]["nodeIds"]
                    .as_array()
                    .map(|a| a.iter().collect::<Vec<_>>())
                    .unwrap_or_else(|| vec![&m["nodeId"]])
            })
            .collect();
        let first = members[0];
        let how = first["details"]["nextAction"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| first["details"]["nextAction"]["how"].as_str().map(str::to_owned))
            .unwrap_or_else(|| {
                "Compare the generated property with the raw Figma value for these nodes (export rawSnapshot with debug: true) before shipping.".into()
            });
        out.push(json!({
            "severity": if lossy || code.starts_with("DEVUP_CODEGEN_INTERACTION_") { "required" } else { "recommended" },
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
    let sources = ["tsx", "componentTsx", "responsiveTsx"].into_iter()
        .filter_map(|field| response[field].as_str()).collect::<Vec<_>>();
    let referenced = sources.iter().flat_map(|source| super::asset_guard::asset_urls(source).into_iter().map(|(url, _)| url))
        .collect::<std::collections::BTreeSet<_>>();
    let written = response["assetManifest"]["assets"].as_array().into_iter().flatten()
        .filter(|asset| asset["status"] == "exported" && asset["outputPath"].is_string())
        .filter_map(|asset| asset["path"].as_str()).collect::<std::collections::BTreeSet<_>>();
    let mut placeholders = response["assetManifest"]["assets"].as_array().into_iter().flatten()
        .filter(|asset| asset["status"] != "exported" || asset["outputPath"].is_null())
        .filter_map(|asset| asset["path"].as_str().map(str::to_owned))
        .collect::<std::collections::BTreeSet<_>>();
    placeholders.extend(referenced.iter().filter(|url| !written.contains(url.as_str())).cloned());
    if !sources.is_empty() {
        placeholders.extend(written.iter().filter(|url| !referenced.contains(**url)).map(|url| (*url).to_owned()));
    }
    if !placeholders.is_empty() {
        out.push(json!({"severity":"required","scope":scope,"code":"ASSET_PLACEHOLDERS",
            "paths":placeholders,"what":"Asset URLs lack a committed output file or a matching generated code reference.",
            "how":"Export the assets with outputPath and assetPublicRoot before using the generated code."}));
    }
    for item in response["responsiveReviewChecklist"]["items"]
        .as_array()
        .into_iter()
        .flatten()
    {
        out.push(json!({"severity":"required","scope":scope,"code":"RESPONSIVE_PAIRING_REVIEW","what":item,
            "how":"Confirm node correspondence at the drawn widths; review undefined intermediate widths with flex, 1fr or minmax rules before claiming responsive completion."}));
    }
    for (field, code, what, how) in [
        (
            "widgetCandidates",
            "WIDGET_INTEGRATION_REQUIRED",
            "External widget candidates require provider integration.",
            "Confirm provider, existing integration and env key names without values; implement and test the widget behavior.",
        ),
        (
            "sharedComponentCandidates",
            "SHARED_COMPONENT_REVIEW",
            "Repeated component instances may differ across screens.",
            "Review instance labels, menu counts and variants together; confirm the authoritative versions, reuse shared components and wire navigation once.",
        ),
    ] {
        if response[field].as_array().is_some_and(|a| !a.is_empty()) {
            out.push(
                json!({"severity":"required","scope":scope,"code":code,"what":what,"how":how}),
            );
        }
    }
    if let Some(issues) = response["projectionIssues"].as_array() {
        items_from_issues(scope, issues, out);
    }
    if response["quality"]["assets"] == "partial" || response["quality"]["assets"] == "failed" {
        let progress = &response["exportJob"]["assetProgress"];
        let requested = progress["total"].as_u64().filter(|total| *total > 0);
        let incomplete = requested.is_none_or(|total| progress["exported"].as_u64() != Some(total));
        out.push(json!({"severity":if incomplete {"required"} else {"info"},"scope":scope,"code":if incomplete {"ASSETS_INCOMPLETE"} else {"UNREQUESTED_ASSETS"},
            "what":"Some assets were not collected.","how":"Read assetSummary.unavailable[]; re-request those assets by id with assetRequests (original url, not artifactId)."}));
    }
    if response["quality"]["assets"] == "complete" && response["assetSummary"]["status"] == "partial" {
        out.push(json!({"severity":"info","scope":scope,"code":"UNREQUESTED_ASSETS",
            "what":"Requested assets were collected; other discovered assets remain unrequested.","how":"Read assetSummary.unavailable[] if those other assets are needed."}));
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
    if let Some(object) = result.as_object_mut() { object.remove("reviewChecklist"); }
    let mut items = vec![];
    if result["selectionEvidence"]["requiresFullDetailRead"] == true {
        items.push(json!({"severity":"required","scope":"response","code":"SELECTED_PROJECTION_DETAILS_RESOURCE",
            "what":"Selection returns data only; full acquisition and projection gaps remain in a linked resource.",
            "how":"Read selectionEvidence.uri before accepting completeness; displayed issue identities are a bounded preview."}));
    }
    if result["metadataDelivery"]["requiresFullDetailRead"] == true {
        items.push(json!({"severity":"required","scope":"response","code":"PROJECTION_DETAILS_RESOURCE",
            "what":"Complete projection diagnostic details are delivered as a resource.",
            "how":"Read metadataDelivery.uri and follow each fullDetailsResource.jsonPointer before accepting evidence; inline issues retain identities and severity but summarize details."}));
    }
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
            items.sort_by_key(|i| {
                (
                    i["severity"] != "required",
                    !text(&i["code"]).starts_with("DEVUP_CODEGEN_INTERACTION_"),
                )
            });
            let omitted = items.len().saturating_sub(MAX_ITEMS);
            items.truncate(MAX_ITEMS);
            object.get_mut("summary").unwrap()["omittedReviewItems"] = json!(omitted);
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
    fn acquired_but_unwritten_assets_require_placeholder_review() {
        let mut value = json!({"status":"complete","quality":{"assets":"complete"},
            "assetManifest":{"assets":[
                {"assetId":"1:1:fills:0","status":"exported","path":"/images/main-hero.webp"},
                {"assetId":"1:2:node","status":"exported","path":"/icons/logo.svg","outputPath":"public/icons/logo.svg"}
            ]}});
        attach(&mut value);
        assert_eq!(value["reviewChecklist"][0]["code"], "ASSET_PLACEHOLDERS");
        assert_eq!(value["reviewChecklist"][0]["severity"], "required");
        assert_eq!(value["reviewChecklist"][0]["paths"], json!(["/images/main-hero.webp"]));
    }

    #[test]
    fn code_only_export_requires_each_unwritten_asset_url() {
        let mut value = json!({"tsx":r#"export const Page = () => <><Image src="/images/photo.webp" /><Box maskImage="url('/icons/arrow.svg')" /><Box bgImage="url(/images/back.webp)" /></>;"#});
        attach(&mut value);
        assert_eq!(value["reviewChecklist"][0]["code"], "ASSET_PLACEHOLDERS");
        assert_eq!(value["reviewChecklist"][0]["severity"], "required");
        assert_eq!(value["reviewChecklist"][0]["paths"], json!(["/icons/arrow.svg", "/images/back.webp", "/images/photo.webp"]));
    }

    #[test]
    fn written_file_without_matching_code_url_requires_review() {
        let mut value = json!({"tsx":r#"export const Page = () => <Image src="/images/placeholder.png" />;"#,
            "assetManifest":{"assets":[{"status":"exported","path":"/images/photo.webp","outputPath":"public/images/photo.webp"}]}});
        attach(&mut value);
        assert_eq!(value["reviewChecklist"][0]["code"], "ASSET_PLACEHOLDERS");
        assert_eq!(value["reviewChecklist"][0]["paths"], json!(["/images/photo.webp", "/images/placeholder.png"]));
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
    #[test]
    fn selected_details_without_preview_findings_still_require_resource_review() {
        let mut value = json!({"status":"partial","selectionEvidence":{"requiresFullDetailRead":true,"uri":"devup://artifact/synthetic/outputs/evidence/manifest"},"projectionIssues":[]});
        attach(&mut value);
        assert_eq!(value["summary"]["required"], 1);
        assert!(
            value["reviewChecklist"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["code"] == "SELECTED_PROJECTION_DETAILS_RESOURCE")
        );
    }
}
