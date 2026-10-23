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
    let candidates = spec_candidates(root);
    if !candidates.is_empty() {
        facts["specCandidates"] = json!(candidates);
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
fn spec_candidates(root: &Path) -> Vec<String> {
    const WORDS: [&str; 9] = [
        "요구사항", "기획", "정의서", "명세", "spec", "requirement", "prd", "srs", "brief",
    ];
    let Ok(entries) = std::fs::read_dir(root) else {
        return vec![];
    };
    let mut found = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let lower = name.to_lowercase();
            let ext = Path::new(&lower).extension()?.to_str()?.to_owned();
            (["pdf", "docx", "md", "hwp", "txt"].contains(&ext.as_str())
                && WORDS.iter().any(|w| lower.contains(w)))
            .then_some(name)
        })
        .collect::<Vec<_>>();
    found.sort();
    found.truncate(10);
    found
}
fn ask(id: &str, prompt: String, options: &[&str]) -> Value {
    question(
        id.into(),
        screen_plan::header("", id)
            .trim_start_matches(": ")
            .to_owned(),
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
        ask("sources", format!("Which optional sources should be read? Figma links, Jira issues/epics, Confluence URLs or files. Spec documents found in the project root: {}. Read them (extract PDF/DOCX text) and pass the text as evidence. Already read evidence: {facts}", facts["specCandidates"]), &["Use recorded sources", "No additional sources"]),
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
pub(super) fn accept_recommended(plan: &mut Value) -> Result<(), DevupError> {
    let fill = array(&plan["questions"])
        .iter()
        .filter(|q| plan["answers"][text(&q["id"])].is_null())
        .filter_map(|q| {
            array(&q["resolve"])
                .first()
                .map(|r| (text(&q["id"]).to_owned(), r["label"].clone()))
        })
        .collect::<BTreeMap<_, _>>();
    answers(plan, &fill)
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
    let mut questions = intake(&plan["facts"])
        .into_iter()
        .filter(|question| plan["answers"][text(&question["id"])].is_null())
        .collect::<Vec<_>>();
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
        let haystack = format!("{} {}", text(&feature["scope"]), feature["acceptance"]).to_lowercase();
        let has_screens = !array(&feature["screens"]).is_empty();
        let topics: [(&str, &str, &[&str], [&str; 3]); 5] = [
            ("screens", "the screens/routes and user roles (the feature lists none)", &[], ["Derive screens from the acceptance criteria", "List screens now", "Exclude this capability"]),
            ("states", "empty, loading and error states", &[], ["Use recorded feature acceptance criteria", "Standard empty/loading/error states", "Exclude this capability"]),
            ("permissions", "permissions and ownership", &["admin", "auth", "login", "role", "permission", "owner", "member"], ["Admins only", "Signed-in users", "Public"]),
            ("forms", "forms and field validation", &["form", "input", "inquiry", "contact", "signup", "register", "submit", "search", "write", "edit"], ["Required fields with inline validation", "Use recorded feature acceptance criteria", "Exclude this capability"]),
            ("notifications", "notifications and external integrations (payment, mail, maps, login providers)", &["mail", "email", "smtp", "notif", "payment", "map", "sms", "alert", "analytics", "oauth"], ["No notifications or integrations", "Email to a staff address", "Other integration (custom answer)"]),
        ];
        for (topic, prompt, keywords, options) in topics {
            let relevant = match topic {
                "screens" => !has_screens,
                "states" => true,
                _ => keywords.iter().any(|k| haystack.contains(k)),
            };
            let key = format!("{id}.{topic}");
            if relevant && plan["facts"][&key].is_null() {
                questions.push(ask(&key, format!("For {} ({id}), specify {prompt}. Screens: {}. Record a concrete decision or explicitly exclude the capability.", text(&feature["scope"]), feature["screens"]), &options));
            }
        }
    }
    if erd::api(plan) && !array(&plan["entities"]).is_empty() {
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
