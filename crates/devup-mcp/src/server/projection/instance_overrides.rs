fn attach_instance_overrides(
    result: &mut Value,
    payload: &CollectedPayload,
    roots: &[String],
) {
    let options = CodegenOptions::default().with_payload_tokens(payload);
    let mut records = Vec::new();
    if let Some(frames) = result.get_mut("frames").and_then(Value::as_array_mut) {
        for frame in frames {
            if let Some(id) = frame["nodeId"].as_str().map(str::to_owned) {
                attach_override_records(frame, devup_mcp_devup_ui::codegen::instance_overrides::instance_overrides(
                    &payload.snapshot, &id, &options));
                if let Some(entries) = frame["instanceOverrides"].as_array() {
                    for entry in entries {
                        if !records.contains(entry) { records.push(entry.clone()); }
                    }
                }
            }
        }
    }
    if ["tsx", "componentTsx", "responsiveTsx"].iter().any(|key| result[*key].is_string()) {
        for record in roots.iter().flat_map(|id| {
            devup_mcp_devup_ui::codegen::instance_overrides::instance_overrides(&payload.snapshot, id, &options)
        }) {
            if !records.contains(&record) { records.push(record); }
        }
    }
    attach_override_records(result, records);
}

fn attach_override_records(result: &mut Value, records: Vec<Value>) {
    if records.is_empty() { return; }
    let issues = result.as_object_mut().map(|object| object.entry("projectionIssues").or_insert_with(|| json!([])));
    if let Some(issues) = issues.and_then(Value::as_array_mut) {
        for record in &records {
            let issue = json!(devup_mcp_devup_ui::codegen::instance_overrides::diagnostic(record.clone()));
            if !issues.iter().any(|existing| existing["code"] == issue["code"] && existing["details"] == issue["details"]) {
                issues.push(issue);
            }
        }
    }
    result["instanceOverrides"] = json!(records);
    if result["deliverable"].is_object() {
        result["deliverable"]["isFinal"] = json!(false);
    }
}
