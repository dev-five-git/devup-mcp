use super::*;

pub(crate) fn written_sources(result: &Value) -> Vec<(String, String)> {
    result["outputPaths"].as_object().into_iter().flat_map(|paths| paths.iter())
        .filter(|(key, _)| matches!(key.rsplit(':').next(), Some("tsx" | "componentTsx" | "responsiveTsx")))
        .filter_map(|(key, path)| Some((key.clone(), std::fs::read_to_string(path.as_str()?).ok()?)))
        .collect()
}

pub(super) fn apply(result: &mut Map<String, Value>, paths: &mut BTreeMap<String, String>, root: Option<&str>, policy: &OutputPolicy) {
    let Some(root) = root else { return };
    let mut defaults = Map::new();
    for (output, extension) in [("tsx", "tsx"), ("componentTsx", "component.tsx"), ("responsiveTsx", "responsive.tsx"), ("sourceMap", "sourcemap.json")] {
        if paths.contains_key(output) { continue; }
        let targets: Vec<(String, String)> = if let Some(frames) = result.get("frames").and_then(Value::as_array) {
            frames.iter().filter(|frame| frame.get(output).is_some()).filter_map(|frame| {
                let id = frame["nodeId"].as_str()?;
                Some((format!("frame:{id}:{output}"), id.into()))
            }).collect()
        } else if result.contains_key(output) {
            let id = result.get("_defaultNodeId").and_then(Value::as_str).unwrap_or("export");
            vec![(output.into(), id.into())]
        } else { vec![] };
        for (key, id) in targets {
            if paths.contains_key(&key) { continue; }
            let path = std::path::Path::new(root).join(".devup/exports").join(format!("{}.{extension}", id.replace([':', ';'], "-")));
            let path = path.to_string_lossy().into_owned();
            if policy.resolve(&path).is_ok() {
                paths.insert(key.clone(), path.clone());
                defaults.insert(key, json!(path));
            } else if let Some(warnings) = result.entry("warnings").or_insert_with(|| json!([])).as_array_mut() {
                warnings.push(json!({"code":"DEVUP_DEFAULT_OUTPUT_PATH_SKIPPED","path":path,"message":"Default output is not allowed; keeping inline delivery."}));
            }
        }
    }
    if !defaults.is_empty() { result.insert("defaultOutputPaths".into(), json!(defaults)); }
}

pub(super) fn omit_written(result: &mut Map<String, Value>) {
    let paths = result.get("outputPaths").and_then(Value::as_object).cloned().unwrap_or_default();
    for (key, path) in paths {
        if let Some(frame_key) = key.strip_prefix("frame:") {
            if let Some((id, output)) = frame_key.rsplit_once(':')
                && matches!(output, "tsx" | "componentTsx" | "responsiveTsx")
                && let Some(frames) = result.get_mut("frames").and_then(Value::as_array_mut) {
                for frame in frames.iter_mut().filter(|frame| frame["nodeId"] == id) {
                    if let Some(object) = frame.as_object_mut() { object.remove(output); }
                    frame["outputResults"][output]["bodyDelivery"] = json!("written-to-file");
                    frame["outputResults"][output]["path"] = path.clone();
                }
            }
        } else if matches!(key.as_str(), "tsx" | "componentTsx" | "responsiveTsx") {
            result.remove(&key);
            let outputs = result.entry("outputResults").or_insert_with(|| json!({}));
            outputs[&key]["bodyDelivery"] = json!("written-to-file");
            outputs[&key]["path"] = path;
        }
    }
}

pub(super) fn compact_written(result: &mut Map<String, Value>, outputs: &mut Vec<ProjectedOutput>, artifact_id: &str) -> Result<(), DevupError> {
    if !result.get("outputPaths").and_then(Value::as_object).is_some_and(|paths| paths.keys().any(|key| matches!(key.rsplit(':').next(), Some("tsx" | "componentTsx" | "responsiveTsx")))) {
        return Ok(());
    }
    let details = ProjectedOutput::text("written-export-details.json", "application/json", serde_json::to_vec(result).map_err(|error| DevupError::new(ErrorCode::DevupInvalidInput, error.to_string(), false))?);
    let uri = details.manifest_uri(artifact_id);
    let issues = result.get("projectionIssues").cloned().unwrap_or_else(|| json!([]));
    let preview = issues.as_array().into_iter().flatten().take(10).map(|issue|json!({"code":issue["code"],"nodeId":issue["nodeId"],"property":issue["property"],"message":issue["message"],"fidelityImpact":issue["fidelityImpact"],"how":issue["details"]["nextAction"].as_str().or_else(|| issue["details"]["nextAction"]["how"].as_str())})).collect::<Vec<_>>();
    result.retain(|key, _| !matches!(key.as_str(), "sectionChecklist" | "imageFillAudit" | "placementContracts" | "projectionReview" | "verdictScope" | "widthBehavior" | "fidelity" | "completionEvidence"));
    result.insert("projectionIssues".into(), json!(preview));
    result.insert("metadataDelivery".into(), json!({"mode":"resource-details","uri":uri,"name":"written-export-details.json","requiresFullDetailRead":true,"projectionIssueCount":issues.as_array().map_or(0, Vec::len)}));
    outputs.push(details);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_fill_each_frame_without_replacing_an_explicit_path() -> anyhow::Result<()> {
        let root = std::env::temp_dir();
        let policy = OutputPolicy::from_roots(vec![root.clone()])?;
        let mut result = json!({"frames":[
            {"nodeId":"1:2","tsx":"first","responsiveTsx":"responsive"},
            {"nodeId":"1:3;4","tsx":"second"}
        ]}).as_object().unwrap().clone();
        let mut paths = BTreeMap::from([("frame:1:2:tsx".into(), "explicit.tsx".into())]);
        apply(&mut result, &mut paths, root.to_str(), &policy);
        assert_eq!(paths["frame:1:2:tsx"], "explicit.tsx");
        assert!(paths["frame:1:3;4:tsx"].ends_with("1-3-4.tsx"));
        assert!(paths["frame:1:2:responsiveTsx"].ends_with("1-2.responsive.tsx"));
        assert_eq!(result["defaultOutputPaths"].as_object().unwrap().len(), 2);
        Ok(())
    }

    #[test]
    fn default_extensions_match_the_output_format() -> anyhow::Result<()> {
        let root = std::env::temp_dir();
        let policy = OutputPolicy::from_roots(vec![root.clone()])?;
        let mut result = json!({"_defaultNodeId":"51:6723","componentTsx":"component","sourceMap":{}}).as_object().unwrap().clone();
        let mut paths = BTreeMap::new();
        apply(&mut result, &mut paths, root.to_str(), &policy);
        assert!(paths["componentTsx"].ends_with("51-6723.component.tsx"));
        assert!(paths["sourceMap"].ends_with("51-6723.sourcemap.json"));
        Ok(())
    }

    #[test]
    fn disallowed_defaults_keep_inline_code() -> anyhow::Result<()> {
        let root = std::env::temp_dir();
        let policy = OutputPolicy::from_roots(vec![root.join(".")])?;
        let mut result = json!({"tsx":"code","_defaultNodeId":"1:2"}).as_object().unwrap().clone();
        let mut paths = BTreeMap::new();
        apply(&mut result, &mut paths, root.parent().and_then(std::path::Path::to_str), &policy);
        assert!(paths.is_empty());
        assert_eq!(result["tsx"], "code");
        assert_eq!(result["warnings"][0]["code"], "DEVUP_DEFAULT_OUTPUT_PATH_SKIPPED");
        Ok(())
    }
}
