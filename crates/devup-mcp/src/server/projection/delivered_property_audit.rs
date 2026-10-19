/// Last code boundary: run after all TSX/map/evidence rewrites and before any
/// file or resource is materialized. New postprocessors cannot silently add or
/// change a property without updating its verified derivation contract.
fn audit_delivered_properties(value: &mut Value) {
    let mut complete = true;
    let mut maps = Map::new();
    let mut frame_issues = Vec::new();
    for field in ["tsx", "componentTsx", "responsiveTsx"] {
        if !value.get(field).is_some_and(Value::is_string) {
            continue;
        }
        if let Some(map) = value["outputResults"][field]
            .as_object_mut()
            .and_then(|o| o.remove("_sourceMap"))
        {
            maps.insert(field.into(), map);
        }
        let contract = value["outputResults"][field]
            .as_object_mut()
            .and_then(|o| o.remove("_propertyContract"))
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        let Some(code) = value[field].as_str() else {
            continue;
        };
        let risks = devup_mcp_devup_ui::ui_layout::layout_risks(code, None)["risks"].clone();
        if let Some(risks) = risks.as_array() {
            if !value["layoutRisks"].is_array() { value["layoutRisks"] = json!([]); }
            for risk in risks {
                let mut risk = risk.clone();
                risk["output"] = json!(field);
                value["layoutRisks"].as_array_mut().unwrap().push(risk);
            }
        }
        let code = value[field].as_str().unwrap();
        let mut missing =
            devup_mcp_devup_ui::provenance::attributes::uncovered_attributes(code, &contract);
        let layout = &value["outputResults"][field]["fidelity"]["layout"];
        if layout["total"]
            .as_u64()
            .zip(layout["covered"].as_u64())
            .is_some_and(|(total, covered)| covered < total)
        {
            missing.push(json!({"reason":"unmapped-source-layout-fields",
                "sourceFields":value["outputResults"][field]["fidelity"]["uncoveredLayout"],
                "propertyMappingVerified":false,
                "message":"Source layout obligations lack verified emitted mappings, even if every generated attribute has provenance."}));
        }
        if field == "responsiveTsx"
            && value["outputResults"][field]["fidelity"]["mergedMappingVerified"] != true
        {
            missing.push(json!({"reason":"unverified-responsive-merge",
                "propertyMappingVerified":false,
                "gaps":value["outputResults"][field]["fidelity"]["mergeGaps"],
                "message":"Read at some width's slot, the merged module does not write that width's own verified tsx; gaps names where."}));
        }
        value["outputResults"][field]["mappingComplete"] = json!(missing.is_empty());
        if missing.is_empty() {
            continue;
        }
        complete = false;
        let issue = json!({"code":"DEVUP_CODEGEN_PROPERTY_UNMAPPED",
            "message":"The final bidirectional contract has uncovered generated attributes or source layout obligations. Existing value-loss classifications remain unchanged.",
            "nodeId":value["nodeId"].as_str().or(value["source"]["nodeId"].as_str()),"severity":"warning","fidelityImpact":"none",
            "details":{"stage":"delivery-property-audit","output":field,"classification":"mapping-incomplete",
                "unmappedProperties":missing,"propertyMappingVerified":false,
                "nextAction":"Add sourceMap or projectionEvidence derivations for these final attributes. Breakpoint-source maps do not establish merged responsive mappings.",
                "evidenceLimit":"No value loss or browser composition measurement is inferred from missing provenance."}});
        if !value["projectionIssues"].is_array() {
            value["projectionIssues"] = json!([]);
        }
        value["projectionIssues"]
            .as_array_mut()
            .unwrap()
            .push(issue);
        if value["outputResults"][field]["projection"] == "exact" {
            value["outputResults"][field]["projection"] = json!("mapping-incomplete");
        }
        value["outputResults"][field]["mappingComplete"] = json!(false);
    }
    if maps.len() > 1 && value.get("sourceMap").is_some() {
        value["sourceMap"]["byOutput"] = Value::Object(maps);
    }
    if let Some(frames) = value.get_mut("frames").and_then(Value::as_array_mut) {
        for frame in frames {
            audit_delivered_properties(frame);
            complete &= frame["mappingComplete"] != false;
            frame_issues.extend(
                frame["projectionIssues"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|d| d["details"]["stage"] == "delivery-property-audit")
                    .cloned(),
            );
        }
    }
    if !frame_issues.is_empty() {
        if !value["projectionIssues"].is_array() {
            value["projectionIssues"] = json!([]);
        }
        value["projectionIssues"]
            .as_array_mut()
            .unwrap()
            .extend(frame_issues);
    }
    value["mappingComplete"] = json!(complete);
    if !complete {
        if value["quality"]["projection"] == "exact" {
            value["quality"]["projection"] = json!("mapping-incomplete");
        }
        if value["status"] == "complete" {
            value["status"] = json!("partial");
        }
        if let Some(deliverable) = value.get_mut("deliverable") {
            deliverable["isFinal"] = json!(false);
            deliverable["note"] = json!(
                "Generated values remain available; review missing provenance in projectionIssues before accepting the output."
            );
        }
        let failures = value["failures"].as_array().cloned().unwrap_or_default();
        let node_id = value["nodeId"]
            .as_str()
            .or(value["source"]["nodeId"].as_str())
            .map(str::to_owned);
        attach_review_and_deliverable(
            value.as_object_mut().unwrap(),
            &["tsx".into(), "componentTsx".into(), "responsiveTsx".into()],
            &failures,
            node_id.as_deref(),
        );
    }
}
