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
        "checklist":["build", "lint", "typecheck", "tests", "devup_ui_validate", "runtime/visual audit", "devup_stack_diff: zero drift"]})
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
        for feature in array(&plan["features"]) {
            let id = text(&feature["id"]);
            let mut previous = "shared".to_owned();
            let layers = if erd::api(plan) {
                vec!["models", "api", "client", "front"]
            } else {
                vec!["front"]
            };
            for layer in layers {
                let uid = format!("{id}-{layer}");
                units.push(unit(
                    uid.clone(),
                    format!("{}: {layer}", text(&feature["scope"])),
                    vec![previous],
                    feature["acceptance"].clone(),
                    layer,
                ));
                previous = uid;
            }
        }
    }
    validate(&units)?;
    for u in &units {
        let brief = format!(
            include_str!("../text/units-split-bf50d8a5.md"),
            text(&u["id"]),
            text(&u["scope"]),
            u["dependencies"],
            text(&plan["unitQuestions"]),
            json!({"answers":plan["answers"],"facts":plan["facts"],"decisions":plan["decisions"],"backendCommands":plan["backendCommands"],"modelsWorkingDirectory":plan["modelsWorkingDirectory"]}),
            erd::mermaid(array(&plan["entities"])),
            plan["screenPlan"],
            json!({"acceptance":u["acceptance"],"checklist":u["checklist"]})
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
pub(super) fn update(input: &ProjectPlanInput, plan: &mut Value) -> Result<(), DevupError> {
    for update in &input.unit_updates {
        confirmed(plan)?;
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
