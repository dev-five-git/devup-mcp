use super::*;

pub(super) fn draft(plan: &mut Value) {
    if !erd::api(plan) {return;}
    let mut mappings = vec![];
    let mut contracts = vec![];
    for feature in array(&plan["features"]) {
        let screens = units::feature_screens(plan, feature);
        let evidence = format!("{} {} {} {}", feature["scope"], feature["screens"], feature["acceptance"], json!(screens)).to_lowercase();
        let mut operations = vec![];
        for (operation, keywords) in [
            ("list", vec!["list", "table", "search", "filter", "pagination", "목록", "검색", "표"]),
            ("get", vec!["detail", "상세"]),
            ("create", vec!["form", "create", "signup", "input", "textarea", "폼", "등록", "문의"]),
            ("update", vec!["edit", "update", "수정"]),
        ] {
            if keywords.iter().any(|word| evidence.contains(word)) { operations.push(operation); }
        }
        let entities = units::feature_entities(plan, feature);
        if operations.is_empty() && entities.is_empty() { continue; }
        if operations.is_empty() { operations.push("list"); }
        mappings.push(json!({"feature":feature["id"],"screens":feature["screens"],"candidateEntities":entities,"requiredOperations":operations,"status":"draft"}));
        for name in entities {
            let Some(entity) = array(&plan["entities"]).iter().find(|entity| entity["name"] == name) else {continue};
            let fields = array(&entity["columns"]).iter().map(|column| json!({
                "name":column["name"],"type":column["type"],"nullable":column["nullable"],"required":column["nullable"] == false,
            })).collect::<Vec<_>>();
            let resource = if name.ends_with('s') {name.clone()} else {format!("{name}s")};
            for operation in &operations {
                let method = match *operation {"create" => "POST", "update" => "PATCH", _ => "GET"};
                let path = if matches!(*operation, "get" | "update") {format!("/{resource}/{{id}}") } else {format!("/{resource}")};
                let request_fields = if matches!(*operation, "create" | "update") {
                    array(&entity["columns"]).iter().filter(|column| column["primary_key"].is_null() && column["default"].is_null())
                        .map(|column| json!({"name":column["name"],"type":column["type"],"nullable":column["nullable"],"required":*operation == "create" && column["nullable"] == false})).collect::<Vec<_>>()
                } else {vec![]};
                contracts.push(json!({"feature":feature["id"],"entity":name,"operation":operation,"method":method,"path":path,
                    "requestFields":request_fields,"responseFields":fields,"status":"draft","pathEvidence":"entity-name convention; confirm API path before implementation"}));
            }
        }
    }
    plan["dataMappings"] = json!(mappings);
    plan["endpointContracts"] = json!(contracts);
}

fn schema_fields(doc: &Value, schema: &Value, depth: usize) -> std::collections::BTreeSet<String> {
    if depth > 16 {return Default::default();}
    if let Some(reference) = schema["$ref"].as_str().and_then(|reference| reference.strip_prefix('#')) {
        return schema_fields(doc, &doc.pointer(reference).cloned().unwrap_or(Value::Null), depth + 1);
    }
    if schema["type"] == "array" {return schema_fields(doc, &schema["items"], depth + 1);}
    let mut fields = schema["properties"].as_object().into_iter().flatten().map(|(name, _)| name.clone()).collect::<std::collections::BTreeSet<_>>();
    for part in array(&schema["allOf"]) {fields.extend(schema_fields(doc, part, depth + 1));}
    fields
}

fn contract_missing(plan: &Value, path: Option<&Path>) -> Vec<Value> {
    let Some(path) = path else {return vec![]};
    let doc = std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()).unwrap_or(Value::Null);
    let mut missing = vec![];
    for contract in array(&plan["endpointContracts"]) {
        let operation = &doc["paths"][text(&contract["path"])][text(&contract["method"]).to_lowercase()];
        let mut reasons = vec![];
        if !operation.is_object() {
            reasons.push("HTTP method/path absent from openapi.json".to_owned());
        } else {
            let request = schema_fields(&doc, &operation["requestBody"]["content"]["application/json"]["schema"], 0);
            let response = operation["responses"].as_object().into_iter().flatten()
                .filter(|(status, _)| status.starts_with('2')).flat_map(|(_, response)| schema_fields(&doc, &response["content"]["application/json"]["schema"], 0)).collect::<std::collections::BTreeSet<_>>();
            for (key, fields) in [("requestFields", request), ("responseFields", response)] {
                for field in array(&contract[key]) {
                    if !fields.contains(text(&field["name"])) {reasons.push(format!("{key} missing {}", text(&field["name"])));}
                }
            }
        }
        if !reasons.is_empty() {missing.push(json!({"feature":contract["feature"],"method":contract["method"],"path":contract["path"],"reason":reasons.join("; ")}));}
    }
    missing
}

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
    let contract_missing = contract_missing(plan, path.as_deref());
    json!({"openapi":path.map(|p| p.display().to_string()),"wired":wired,"missing":missing,"deferred":deferred,"contractMissing":contract_missing})
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
