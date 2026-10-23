use super::*;

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
    json!({"openapi":path.map(|p| p.display().to_string()),"wired":wired,"missing":missing,"deferred":deferred})
}

pub(super) fn blockers(plan: &Value, root: &str) -> Vec<String> {
    if array(&plan["apiWiring"]).is_empty() {
        return vec![];
    }
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
    missing.chain(deferred).collect()
}

fn declares(cargo: &str, name: &str) -> bool {
    let mut runtime = false;
    cargo.lines().any(|line| {
        let line = line.trim();
        if line.starts_with('[') {
            runtime = line == "[dependencies]" || (line.starts_with("[target.") && line.ends_with(".dependencies]"));
        }
        runtime && line.strip_prefix(name).is_some_and(|rest| {
            rest.starts_with('.') || rest.trim_start().starts_with('=')
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
    if let Ok(cargo) = std::fs::read_to_string(base.join("Cargo.toml")) {
        for dep in ["serde", "serde_json"] {
            if !declares(&cargo, dep) {
                found.push(format!("{dir}/Cargo.toml lacks {dep}; exported entities do not compile without it"));
            }
        }
    }
    if let Ok(main) = std::fs::read_to_string(base.join("src/main.rs"))
        && !syn::parse_file(&main).is_ok_and(|file| file.items.iter().any(|item| matches!(item, syn::Item::Mod(module) if module.ident == "models")))
    {
        found.push(format!("{dir}/src/main.rs lacks `mod models;`; export does not wire src/models"));
    }
    found
}
