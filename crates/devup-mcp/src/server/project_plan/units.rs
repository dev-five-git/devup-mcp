use super::*;

fn unit(
    id: String,
    scope: String,
    dependencies: Vec<String>,
    acceptance: Value,
    layer: &str,
) -> Value {
    json!({"id":id,"scope":scope,"dependencies":dependencies,"acceptance":acceptance,
        "layer":layer,"state":"pending","verification":[],
        "stage":if matches!(layer, "shared" | "models") {0} else {1},
        "checklist":["build", "lint", "typecheck", "tests", "devup_ui_validate", "runtime/visual audit", "devup_stack_diff: zero drift"]})
}
fn feature_entities(plan: &Value, feature: &Value) -> Vec<String> {
    if !erd::api(plan) {
        return vec![];
    }
    let explicit = array(&feature["entities"])
        .iter()
        .map(|v| text(v).to_owned())
        .collect::<Vec<_>>();
    if !explicit.is_empty() {
        return explicit;
    }
    let haystack = format!(
        "{} {} {}",
        text(&feature["id"]),
        text(&feature["scope"]),
        feature["acceptance"]
    )
    .to_lowercase();
    array(&plan["entities"])
        .iter()
        .map(|e| text(&e["name"]).to_owned())
        .filter(|name| haystack.contains(&name.to_lowercase()))
        .collect()
}
fn belongs(key: &str, owner: &str) -> bool {
    !key.contains('.') || key.starts_with(&format!("{owner}."))
}
fn unit_owner(u: &Value) -> String {
    let feature = text(&u["feature"]);
    if feature.is_empty() { text(&u["id"]) } else { feature }.to_owned()
}
fn pick(source: &Value, owner: &str, only: &[&str]) -> Value {
    let map = source.as_object().cloned().unwrap_or_default();
    Value::Object(
        map.into_iter()
            .filter(|(k, _)| belongs(k, owner) && (only.is_empty() || only.contains(&k.as_str()) || k.contains('.')))
            .collect(),
    )
}
fn slim_context(plan: &Value, u: &Value) -> Value {
    let owner = unit_owner(u);
    json!({
        "fullPlan": ".devup/project-plan.json",
        "answers": pick(&plan["answers"], &owner, &[]),
        "scopePrecedence": u["scopePrecedence"],
        "scopeOverrides": u["scopeOverrides"],
        "facts": pick(&plan["facts"], &owner, &["goal", "parts", "workType"]),
        "decisions": plan["decisions"],
        "backendCommands": plan["backendCommands"],
        "backendNotes": plan["backendNotes"],
        "modelsWorkingDirectory": plan["modelsWorkingDirectory"],
    })
}
fn unit_entities(plan: &Value, u: &Value) -> Vec<Value> {
    let names = array(&u["entities"]);
    array(&plan["entities"])
        .iter()
        .filter(|e| names.is_empty() || names.contains(&e["name"]))
        .cloned()
        .collect()
}
fn slim_screens(screen: &Value, screens: &[Value]) -> Value {
    if screen.is_null() {
        return Value::Null;
    }
    let routes = array(&screen["routes"])
        .iter()
        .filter(|route| screens.contains(&route["url"]) || array(&route["nodeIds"]).iter().any(|node| screens.contains(node)))
        .map(|r| json!({"url": r["url"], "nodeIds": r["nodeIds"], "file": r["file"]}))
        .collect::<Vec<_>>();
    json!({"fullPlan": ".devup/screen-plan.json", "state": screen["state"], "routes": routes})
}
fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn validate(units: &[Value]) -> Result<(), DevupError> {
    let mut ids = std::collections::BTreeSet::new();
    for u in units {
        if !safe_id(text(&u["id"])) || !ids.insert(text(&u["id"])) {
            return Err(invalid("Work unit ids must be unique safe filenames"));
        }
    }
    let mut done = std::collections::BTreeSet::new();
    loop {
        let before = done.len();
        for u in units {
            if array(&u["dependencies"])
                .iter()
                .all(|d| done.contains(text(d)))
            {
                done.insert(text(&u["id"]));
            }
        }
        if done.len() == units.len() {
            return Ok(());
        }
        if before == done.len() {
            return Err(invalid("Unit dependencies contain a cycle or unknown id"));
        }
    }
}
pub(super) fn split(
    input: &ProjectPlanInput,
    plan: &mut Value,
    tx: &mut OutputTransaction,
    policy: &OutputPolicy,
) -> Result<(), DevupError> {
    if !array(&plan["units"]).is_empty() {
        return Err(invalid(
            "Plan already split; status updates preserve worker sessions",
        ));
    }
    let mut units = Vec::new();
    let issues: Vec<Issue> =
        serde_json::from_value(plan["issues"].clone()).map_err(|e| invalid(e.to_string()))?;
    if !issues.is_empty() {
        for issue in issues {
            units.push(unit(
                issue.key,
                issue.scope,
                issue.dependencies,
                json!(issue.acceptance),
                "issue",
            ));
        }
    } else {
        units.push(unit(
            "shared".into(),
            "Shared layout, theme and tokens".into(),
            vec![],
            json!(["Shared foundations verified"]),
            "shared",
        ));
        let mut model_units = std::collections::BTreeSet::new();
        for feature in array(&plan["features"]) {
            let id = text(&feature["id"]);
            let used = feature_entities(plan, feature);
            let mut previous = vec!["shared".to_owned()];
            if !used.is_empty() {
                for name in &used {
                    let mid = format!("model-{name}");
                    if model_units.insert(mid.clone()) {
                        let mut u = unit(
                            mid.clone(),
                            format!("Table {name}: model, migration and entity"),
                            vec!["shared".into()],
                            json!(["Model validates and migrates"]),
                            "models",
                        );
                        u["entities"] = json!([name]);
                        units.push(u);
                    }
                }
                previous = used.iter().map(|n| format!("model-{n}")).collect();
            }
            let layers = if used.is_empty() {
                vec!["front"]
            } else {
                vec!["api", "client", "front"]
            };
            for layer in layers {
                let uid = format!("{id}-{layer}");
                let mut u = unit(
                    uid.clone(),
                    format!("{}: {layer}", text(&feature["scope"])),
                    previous,
                    feature["acceptance"].clone(),
                    layer,
                );
                u["feature"] = json!(id);
                u["entities"] = json!(used);
                units.push(u);
                previous = vec![uid];
            }
        }
    }
    let dependencies = units.iter().filter(|unit| unit["stage"] == 1)
        .map(|unit| text(&unit["id"]).to_owned()).collect();
    let mut integration = unit("integration".into(), "Supervisor integration and final verification".into(),
        dependencies, json!(["All worker outputs integrated and verified"]), "integration");
    integration["stage"] = json!(2);
    integration["owner"] = json!("supervisor");
    units.push(integration);
    validate(&units)?;
    let mut screen_context = plan["screenPlan"].clone();
    if let Some(context) = screen_context.as_object_mut() {
        for key in ["questions", "questionRounds", "instruction", "askInstruction"] {
            context.remove(key);
        }
    }
    for u in &mut units {
        if u["stage"] == 0 {
            u["owner"] = json!("supervisor");
        }
        let owner = unit_owner(u);
        u["scopeOverrides"] = pick(&plan["answers"], &owner, &[]);
        u["scopePrecedence"] = json!(["answers", "decisions", "scope"]);
        let screens = array(&plan["features"]).iter().find(|feature| feature["id"] == owner)
            .map(|feature| array(&feature["screens"])).unwrap_or_default();
        let screen_slice = slim_screens(&screen_context, screens);
        let mut contract = json!({"routes":screen_slice["routes"],"protectedFiles":plan["facts"]["protectedFiles"]});
        for field in ["exports", "assets", "imports"] {
            contract[field] = plan["facts"][format!("{owner}.{field}")].clone();
        }
        u["contract"] = contract;
        let brief = format!(
            include_str!("../text/units-split-bf50d8a5.md"),
            text(&u["id"]),
            text(&u["scope"]),
            u["dependencies"],
            text(&plan["unitQuestions"]),
            slim_context(plan, u),
            erd::mermaid(&unit_entities(plan, u)),
            screen_slice,
            json!({"acceptance":u["acceptance"],"checklist":u["checklist"],"contract":u["contract"]})
        );
        stage(
            tx,
            policy,
            &input.project_root,
            &format!(".devup/units/{}.md", text(&u["id"])),
            brief.as_bytes(),
        )?;
    }
    plan["units"] = json!(units);
    Ok(())
}
pub(super) fn open_requests(plan: &Value) -> Vec<Value> {
    array(&plan["sharedChangeRequests"])
        .iter()
        .filter(|r| r["state"] == "open")
        .cloned()
        .collect()
}
fn shared_request(plan: &mut Value, update: &UnitUpdate) -> Result<(), DevupError> {
    let file = update.file.as_deref().unwrap_or_default().trim();
    if file.is_empty() {
        return Err(invalid("shared-change-request needs file"));
    }
    if !plan["sharedChangeRequests"].is_array() {
        plan["sharedChangeRequests"] = json!([]);
    }
    if update.state == "resolved" {
        for r in plan["sharedChangeRequests"].as_array_mut().unwrap() {
            if r["file"] == file && r["state"] == "open" {
                r["state"] = json!("resolved");
            }
        }
        for u in plan["units"].as_array_mut().unwrap() {
            if u["state"] == "merged" && u["stage"] == 1 {
                u["state"] = json!("verifying");
            }
        }
        return Ok(());
    }
    if update.reason.as_deref().is_none_or(|r| r.trim().is_empty()) {
        return Err(invalid("shared-change-request needs reason"));
    }
    plan["sharedChangeRequests"].as_array_mut().unwrap().push(json!({
        "unit":update.id,"file":file,"reason":update.reason,"diff":update.diff,"state":"open"}));
    Ok(())
}
pub(super) fn update(input: &ProjectPlanInput, plan: &mut Value) -> Result<(), DevupError> {
    for update in &input.unit_updates {
        confirmed(plan)?;
        if update.kind.as_deref() == Some("shared-change-request") {
            shared_request(plan, update)?;
            continue;
        }
        let current = array(&plan["units"])
            .iter()
            .find(|u| u["id"] == update.id)
            .ok_or_else(|| invalid("Unknown unit"))?;
        let old = text(&current["state"]);
        let valid = match (old, update.state.as_str()) {
            (a, b) if a == b => true,
            ("pending", "running") => array(&current["dependencies"]).iter().all(|d| {
                array(&plan["units"])
                    .iter()
                    .any(|u| u["id"] == *d && u["state"] == "merged")
            }),
            ("running", "verifying" | "unresolved")
            | ("verifying", "running" | "unresolved")
            | ("unresolved", "running") => true,
            ("verifying", "merged") => !update.verification.is_empty(),
            _ => false,
        };
        if !valid {
            return Err(invalid(
                "Invalid unit state transition or missing supervisor verification/dependencies",
            ));
        }
        let u = plan["units"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|u| u["id"] == update.id)
            .unwrap();
        u["state"] = json!(update.state);
        u["verification"] = json!(update.verification);
    }
    Ok(())
}
pub(super) fn dag(units: &[Value]) -> String {
    let mut out = "flowchart TD\n".to_owned();
    for u in units {
        let id = text(&u["id"]);
        out.push_str(&format!("  {id}[{id}]\n"));
        for d in array(&u["dependencies"]) {
            out.push_str(&format!("  {} --> {id}\n", text(d)));
        }
    }
    out
}
