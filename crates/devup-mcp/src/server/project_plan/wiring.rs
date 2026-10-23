use super::*;
mod draft;
mod contract;
mod schema;
pub(super) use draft::draft;



fn openapi_path(root: &str) -> Option<std::path::PathBuf> {
    let base = Path::new(root);
    let mut candidates = vec![base.join("openapi.json")];
    if let Ok(entries) = std::fs::read_dir(base.join("apis")) {
        candidates.extend(
            entries
                .filter_map(Result::ok)
                .map(|e| e.path().join("openapi.json")),
        );
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn operation_ids(path: &Path) -> std::collections::BTreeSet<String> {
    let oversized = std::fs::metadata(path).is_ok_and(|m| m.len() > 16 * 1024 * 1024);
    let doc = std::fs::read(path)
        .ok()
        .filter(|_| !oversized)
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .unwrap_or(Value::Null);
    doc["paths"]
        .as_object()
        .into_iter()
        .flatten()
        .flat_map(|(_, item)| item.as_object().into_iter().flatten())
        .filter(|(method, _)| matches!(method.as_str(), "get" | "post" | "put" | "patch" | "delete" | "head" | "options" | "trace"))
        .filter_map(|(_, op)| op["operationId"].as_str().map(str::to_owned))
        .collect()
}

pub(super) fn record(plan: &mut Value, wiring: &[ApiWiring]) -> Result<(), DevupError> {
    for w in wiring {
        let known = array(&plan["features"]);
        if w.feature.trim().is_empty() || (!known.is_empty() && !known.iter().any(|f| f["id"] == w.feature)) {
            return Err(invalid(format!("apiWiring names unknown feature {}", w.feature)));
        }
        let operation = w.operation_id.as_deref().map(str::trim).unwrap_or_default();
        if w.defer {
            if w.reason.as_deref().is_none_or(|r| r.trim().is_empty()) {
                return Err(invalid("A deferred API wiring needs a reason; it is reported as unfinished"));
            }
        } else if operation.is_empty() {
            return Err(invalid("apiWiring needs operationId, or defer true with a reason"));
        }
        if !plan["apiWiring"].is_array() {
            plan["apiWiring"] = json!([]);
        }
        let list = plan["apiWiring"].as_array_mut().unwrap();
        list.retain(|v| v["feature"] != w.feature);
        list.push(json!(w));
    }
    Ok(())
}

pub(super) fn report(plan: &Value, root: &str) -> Value {
    let path = openapi_path(root);
    let ids = path.as_deref().map(operation_ids).unwrap_or_default();
    let (mut wired, mut missing, mut deferred) = (vec![], vec![], vec![]);
    for feature in array(&plan["features"]).iter().filter(|feature| !array(&feature["entities"]).is_empty() || !units::feature_entities(plan, feature).is_empty() || array(&plan["dataMappings"]).iter().any(|mapping| mapping["feature"] == feature["id"])) {
        if !array(&plan["apiWiring"]).iter().any(|w| w["feature"] == feature["id"]) {
            missing.push(json!({"feature":feature["id"],"operationId":null,"reason":"Data feature has no operationId binding"}));
        }
    }
    for w in array(&plan["apiWiring"]) {
        if w["defer"] == true {
            deferred.push(json!({"feature":w["feature"],"reason":w["reason"]}));
        } else if ids.contains(text(&w["operationId"])) {
            wired.push(json!({"feature":w["feature"],"operationId":w["operationId"]}));
        } else {
            let why = if path.is_none() {
                "openapi.json not found"
            } else {
                "operationId is not in openapi.json"
            };
            missing.push(json!({"feature":w["feature"],"operationId":w["operationId"],"reason":why}));
        }
    }
    let (contract_missing, contract_matches) = contract::report(plan, path.as_deref());
    json!({"openapi":path.map(|p| p.display().to_string()),"wired":wired,"missing":missing,"deferred":deferred,"contractMissing":contract_missing,"contractMatches":contract_matches})
}

pub(super) fn blockers(plan: &Value, root: &str) -> Vec<String> {
    let r = report(plan, root);
    let missing = array(&r["missing"]).iter().map(|m| {
        format!(
            "apiWiring: {} -> {} ({})",
            text(&m["feature"]),
            text(&m["operationId"]),
            text(&m["reason"])
        )
    });
    let deferred = array(&r["deferred"]).iter().map(|d| {
        format!(
            "unfinished: {} API wiring deferred ({})",
            text(&d["feature"]),
            text(&d["reason"])
        )
    });
    let contracts = array(&r["contractMissing"]).iter().map(|contract| format!("endpointContract: {} {} ({})", text(&contract["method"]), text(&contract["path"]), text(&contract["reason"])));
    missing.chain(deferred).chain(contracts).collect()
}

fn cargo_document(path: &Path) -> Option<toml_edit::DocumentMut> {
    std::fs::read_to_string(path).ok()?.parse().ok()
}

fn workspace_document(base: &Path, cargo: &toml_edit::DocumentMut) -> Option<toml_edit::DocumentMut> {
    if let Some(workspace) = cargo.get("package").and_then(|p| p.get("workspace")).and_then(toml_edit::Item::as_str) {
        return cargo_document(&base.join(workspace).join("Cargo.toml"));
    }
    base.ancestors().find_map(|ancestor| {
        cargo_document(&ancestor.join("Cargo.toml")).filter(|doc| doc.get("workspace").is_some())
    })
}

fn has_derive(dep: &toml_edit::Item) -> bool {
    dep.get("features").and_then(toml_edit::Item::as_array)
        .is_some_and(|features| features.iter().any(|feature| feature.as_str() == Some("derive")))
}

fn declares(cargo: &toml_edit::DocumentMut, workspace: Option<&toml_edit::DocumentMut>, name: &str) -> bool {
    let mut tables = vec![cargo.get("dependencies")];
    if let Some(targets) = cargo.get("target").and_then(toml_edit::Item::as_table_like) {
        tables.extend(targets.iter().map(|(_, target)| target.get("dependencies")));
    }
    tables.into_iter().flatten().filter_map(toml_edit::Item::as_table_like)
        .flat_map(|table| table.iter()).any(|(key, dep)| {
            let inherited = dep.get("workspace").and_then(toml_edit::Item::as_bool) == Some(true);
            let resolved = if inherited {
                workspace.and_then(|doc| doc.get("workspace")?.get("dependencies")?.get(key))
            } else { Some(dep) };
            resolved.is_some_and(|resolved| {
                let package = resolved.get("package").and_then(toml_edit::Item::as_str).unwrap_or(key);
                package == name && (name != "serde" || has_derive(dep) || has_derive(resolved))
            })
        })
}

pub(super) fn backend_checks(plan: &Value, root: &str) -> Vec<String> {
    let dir = text(&plan["modelsWorkingDirectory"]);
    if plan["state"] != "confirmed" || dir.is_empty() {
        return vec![];
    }
    let base = Path::new(root).join(dir);
    let mut found = vec![];
    if let Some(cargo) = cargo_document(&base.join("Cargo.toml")) {
        let workspace = workspace_document(&base, &cargo);
        for (dep, command) in [("serde", "cargo add serde --features derive"), ("serde_json", "cargo add serde_json")] {
            if !declares(&cargo, workspace.as_ref(), dep) {
                found.push(format!("{dir}/Cargo.toml lacks {dep}{}; run `{command}` in {dir}", if dep == "serde" { " with derive" } else { "" }));
            }
        }
    } else if base.join("Cargo.toml").is_file() {
        found.push(format!("{dir}/Cargo.toml cannot be parsed as TOML; fix the manifest before checking exported entity dependencies"));
    }
    if let Ok(main) = std::fs::read_to_string(base.join("src/main.rs"))
        && !syn::parse_file(&main).is_ok_and(|file| file.items.iter().any(|item| matches!(item, syn::Item::Mod(module) if module.ident == "models")))
    {
        found.push(format!("{dir}/src/main.rs lacks `mod models;`; export does not wire src/models"));
    }
    found
}
