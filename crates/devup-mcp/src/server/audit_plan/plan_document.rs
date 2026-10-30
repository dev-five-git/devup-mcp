fn write_component_substitutions(
    plan_path: &Path,
    policy: &OutputPolicy,
    substitutions: &[Value],
) -> Result<(), DevupError> {
    if !substitutions.is_empty() {
        let bytes = std::fs::read(plan_path).map_err(|error| invalid(error.to_string()))?;
        let mut document: Value = serde_json::from_slice(&bytes).map_err(|error| invalid(error.to_string()))?;
        document["componentSubstitutions"] = json!(substitutions);
        let target = policy.resolve(&plan_path.to_string_lossy())?;
        let mut transaction = super::output::OutputTransaction::new();
        transaction.stage("componentSubstitutions", target, &serde_json::to_vec_pretty(&document).map_err(|error| invalid(error.to_string()))?)?;
        transaction.commit()?;
    }
    Ok(())
}

const REFERENCES_DIR: &str = ".devup/runtime-audit/references";

/// Writes each frame's referencePng beside the audit plans, so a page
/// comparison can still run as `reference: {path}` after the in-memory
/// artifact has expired or the server restarted.
fn persist_references(
    root: &str,
    sources: &Sources,
    policy: &OutputPolicy,
) -> Result<Vec<Value>, DevupError> {
    use base64::Engine as _;
    let mut transaction = super::output::OutputTransaction::new();
    let mut written = Vec::new();
    for (artifact_id, payload) in &sources.payloads {
        let (Some(node_id), Some(reference)) =
            (payload.target.node_id.as_deref(), payload.reference_png.as_ref())
        else {
            continue;
        };
        let Ok(bytes) =
            base64::engine::general_purpose::STANDARD.decode(&reference.data_base64)
        else {
            continue;
        };
        let path = Path::new(root)
            .join(REFERENCES_DIR)
            .join(format!("{}.png", slug(node_id)));
        let target = policy.resolve(&path.to_string_lossy())?;
        transaction.stage(format!("reference:{node_id}"), target, &bytes)?;
        written.push(json!({"nodeId": node_id, "artifactId": artifact_id,
            "path": path.display().to_string(), "sha256": reference.sha256,
            "compareWith": {"reference": {"path": path.display().to_string()}}}));
    }
    if !written.is_empty() {
        transaction.commit()?;
    }
    Ok(written)
}

fn stored_references(root: &str) -> Vec<String> {
    let mut found = std::fs::read_dir(Path::new(root).join(REFERENCES_DIR))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "png"))
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    found.sort();
    found
}

fn local_imports(source: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("import ") else {
            continue;
        };
        let Some((names, module)) = rest.rsplit_once(" from ") else {
            continue;
        };
        let module = module
            .trim()
            .trim_end_matches(';')
            .trim_matches(['"', '\'']);
        if module.starts_with("./") || module.starts_with("../") || module.starts_with("@/") {
            found.push((names.to_owned(), module.to_owned()));
        }
    }
    found
}
