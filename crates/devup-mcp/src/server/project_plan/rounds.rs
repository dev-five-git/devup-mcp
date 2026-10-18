use super::*;
use screen_plan::{choice, question};

pub(super) fn facts(input: &ProjectPlanInput) -> Result<Value, DevupError> {
    let root = Path::new(&input.project_root);
    let mut facts = json!({});
    if root.join("apis").is_dir() {
        facts["parts"] = json!("front + admin + API");
        facts["partsEvidence"] = json!("apis/ exists");
    } else if root.join("apps/admin").is_dir() {
        facts["parts"] = json!("front + admin");
        facts["partsEvidence"] = json!("apps/admin exists");
    }
    for ev in &input.evidence {
        if ev.source.trim().is_empty() {
            return Err(invalid("Evidence requires a source"));
        }
        for (k, v) in &ev.facts {
            facts[k] = v.clone();
        }
    }
    Ok(facts)
}
fn ask(id: &str, prompt: String, options: &[&str]) -> Value {
    question(
        id.into(),
        screen_plan::header("", id),
        prompt,
        options
            .iter()
            .map(|label| {
                choice(
                    *label,
                    format!("Record {label} for this project; custom answers accepted"),
                    json!(label),
                )
            })
            .collect(),
        "project",
    )
}
pub(super) fn intake(facts: &Value) -> Vec<Value> {
    let parts = text(&facts["parts"]);
    vec![
        ask("workType", "What kind of work is this? Choose new project, existing feature, bug fix, Figma page implementation, or refactor (custom answer).".into(), &["new project", "feature on existing project", "bug fix", "Figma page implementation"]),
        ask("sources", format!("Which optional sources should be read? Figma links, Jira issues/epics, Confluence URLs or files. Already read evidence: {facts}"), &["Use recorded sources", "No additional sources"]),
        ask("goal", format!("What should be built in one sentence? Source default: {}", text(&facts["goal"])), &[if facts["goal"].is_string() {text(&facts["goal"])} else {"Describe the goal"}, "Use source scope"]),
        ask("parts", format!("Which parts are in scope? Default evidence: {}", facts["partsEvidence"]), &[if parts.is_empty() {"front only"} else {parts}, "front + admin", "front + admin + API"]),
    ]
}
pub(super) fn answers(
    plan: &mut Value,
    answers: &BTreeMap<String, Value>,
) -> Result<(), DevupError> {
    for (id, value) in answers {
        if !array(&plan["questions"]).iter().any(|q| q["id"] == *id) {
            return Err(invalid(format!("Unknown question {id}")));
        }
        if value.as_str().is_none_or(|s| s.trim().is_empty()) {
            return Err(invalid(format!("Answer {id} needs nonempty text")));
        }
        let answer = screen_plan::strip_recommended(value.as_str().unwrap());
        if id == "goal"
            && ["Describe the goal", "Use source scope"].contains(&answer)
            && !plan["facts"]["goal"].is_string()
        {
            return Err(invalid("Provide a concrete one-sentence goal"));
        }
        plan["answers"][id] = json!(answer);
        if ["parts", "goal", "workType"].contains(&id.as_str())
            && (id != "goal" || answer != "Use source scope")
        {
            plan["facts"][id] = json!(answer);
        }
    }
    Ok(())
}
pub(super) fn require_answers(plan: &Value) -> Result<(), DevupError> {
    let missing = array(&plan["questions"])
        .iter()
        .filter(|q| plan["answers"][text(&q["id"])].is_null())
        .map(|q| text(&q["id"]))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(invalid(format!(
            "Answer all intake rounds first: {}",
            missing.join(", ")
        )))
    }
}
pub(super) fn detail(
    plan: &Value,
    extra: &[screen_plan::ExtraQuestionInput],
) -> Result<Vec<Value>, DevupError> {
    let mut questions = vec![];
    if array(&plan["features"]).is_empty() && array(&plan["issues"]).is_empty() {
        return Err(invalid(
            "draft needs features or Jira issues read from sources",
        ));
    }
    for feature in array(&plan["features"])
        .iter()
        .chain(array(&plan["issues"]))
    {
        let id = feature["id"]
            .as_str()
            .or_else(|| feature["key"].as_str())
            .unwrap_or("feature");
        for (topic, prompt) in [
            ("screens", "screens/routes and user roles"),
            ("states", "empty, loading and error states"),
            ("permissions", "permissions and ownership"),
            ("forms", "forms and field validation"),
            (
                "notifications",
                "notifications and external integrations (payment, mail, maps, login providers)",
            ),
        ] {
            let key = format!("{id}.{topic}");
            if plan["facts"][&key].is_null() {
                questions.push(ask(&key, format!("For {} ({id}), specify {prompt}. Screens: {}. Record a concrete decision or explicitly exclude the capability.", text(&feature["scope"]), feature["screens"]), &["Use recorded feature acceptance criteria", "Exclude this capability"]));
            }
        }
    }
    if erd::api(plan) {
        for (key, topic) in [
            (
                "erd",
                "Review every entity, column, required/nullable field, relation and N:M join table",
            ),
            ("tenancy", "Set ownership and multi-tenancy boundaries"),
            (
                "auth",
                "Confirm user/auth model, providers and gapped integer roles",
            ),
            (
                "lifecycle",
                "Confirm status enums, soft delete, audit timestamps and deletion actions",
            ),
            (
                "indexes",
                "Confirm unique/composite keys and indexes for each listed filter/sort",
            ),
            ("storage", "Set file/image storage and nullable metadata"),
        ] {
            if plan["facts"][key].is_null() {
                questions.push(ask(key, format!("{topic}. Draft ERD: {}. Supply changes as entities/relations with confirm; a prose correction requires redraft.", plan["entities"]), &["Approve recorded ERD", "Revise ERD before confirming"]));
            }
        }
    }
    for (key, options) in [
        ("jiraPolicy", vec!["none", "comment", "transition"]),
        (
            "branchPolicy",
            vec!["Use current branch", "Create feature branch"],
        ),
        ("prPolicy", vec!["No PR", "Create draft PR"]),
    ] {
        if plan["facts"][key].is_null() {
            questions.push(ask(
                key,
                format!("Delivery: choose {key} for {}", plan["facts"]["goal"]),
                &options,
            ));
        }
    }
    questions.extend(screen_plan::extra_questions(extra)?);
    let mut seen = std::collections::BTreeSet::new();
    if questions.iter().any(|q| !seen.insert(text(&q["id"]))) {
        return Err(invalid("Question ids must be unique"));
    }
    Ok(questions)
}
