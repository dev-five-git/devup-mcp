//! Project intake and supervisor state. Source reads are supplied by the runtime;
//! local evidence is collected before generating any unresolved questions.
mod erd;
mod rounds;
#[cfg(test)]
mod tests;
mod units;
mod wiring;

use super::{
    output::{OutputPolicy, OutputTransaction},
    screen_plan,
};
use devup_mcp_figma::DevupError;
use schemars::JsonSchema;
use screen_plan::{array, invalid, now, sha256, text};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

const FILE: &str = ".devup/project-plan.json";
const KIND: &str = "devup-project-plan";

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct ProjectPlanInput {
    #[schemars(extend("enum" = ["intake", "draft", "confirm", "status", "assume", "defer", "split"]))]
    pub action: String,
    pub project_root: String,
    pub plan_id: Option<String>,
    /// Facts read from Jira issues/epic children, Figma exploration, spec documents,
    /// and devup_project_context all scopes. Every fact includes its source.
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    #[schemars(extend("additionalProperties" = json!({})))]
    pub answers: BTreeMap<String, Value>,
    /// draft/confirm: the user accepted the proposal table; every question
    /// missing from `answers` takes its recommended option.
    #[serde(default)]
    pub accept_recommended: bool,
    #[serde(default)]
    pub features: Vec<Feature>,
    /// Explicit ERD models in vespertide wire format; confirm may refine the draft.
    #[serde(default)]
    #[schemars(extend("items" = json!({})))]
    pub entities: Vec<Value>,
    #[serde(default)]
    pub relations: Vec<Relation>,
    #[serde(default)]
    pub issues: Vec<Issue>,
    #[serde(default)]
    pub extra_questions: Vec<screen_plan::ExtraQuestionInput>,
    pub unit_questions: Option<String>,
    pub models_dir: Option<String>,
    #[serde(default)]
    pub decisions: Vec<Decision>,
    #[serde(default)]
    pub unit_updates: Vec<UnitUpdate>,
    /// Records the operationId each feature's pages wire to ("Wire to the API
    /// now"); status checks every id against openapi.json.
    #[serde(default)]
    pub api_wiring: Vec<ApiWiring>,
    /// summary (default) answers with state, open questions, next action, ERD and
    /// unit DAG; full also returns the whole plan.
    #[schemars(extend("enum" = ["summary", "full"]))]
    pub detail: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct Evidence {
    pub source: String,
    #[schemars(extend("additionalProperties" = json!({})))]
    pub facts: BTreeMap<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct Feature {
    pub id: String,
    pub scope: String,
    #[serde(default)]
    pub screens: Vec<String>,
    #[serde(default)]
    pub acceptance: Vec<String>,
    /// Tables this feature reads or writes; split makes one model unit per table.
    #[serde(default)]
    pub entities: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct Issue {
    pub key: String,
    pub scope: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub acceptance: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct Relation {
    pub from: String,
    pub to: String,
    /// oneToMany or manyToMany. from owns the referenced id, to owns the FK.
    pub cardinality: String,
    pub column: Option<String>,
    pub join_table: Option<String>,
    #[serde(default = "restrict")]
    pub on_delete: String,
}
fn restrict() -> String {
    "restrict".into()
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct Decision {
    pub id: String,
    pub reason: String,
    pub recommended: String,
    #[serde(default)]
    pub destructive: bool,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct UnitUpdate {
    pub id: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub verification: Vec<String>,
    /// "shared-change-request": a worker asks the supervisor to change a stage 0
    /// file. Needs file and reason; state "resolved" closes requests for that file.
    pub kind: Option<String>,
    pub file: Option<String>,
    pub reason: Option<String>,
    pub diff: Option<String>,
}
/// Binds a feature to the API operation its pages call. `defer` records the
/// explicit decision to use no API yet; it is reported as unfinished.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct ApiWiring {
    pub feature: String,
    pub operation_id: Option<String>,
    #[serde(default)]
    pub defer: bool,
    pub reason: Option<String>,
}

fn read(root: &str) -> Result<Value, DevupError> {
    let path = Path::new(root).join(FILE);
    let metadata =
        std::fs::metadata(&path).map_err(|e| invalid(format!("Read project plan: {e}")))?;
    if metadata.len() > 2 * 1024 * 1024 {
        return Err(invalid("Project plan exceeds 2 MiB"));
    }
    let plan: Value =
        serde_json::from_slice(&std::fs::read(path).map_err(|e| invalid(e.to_string()))?)
            .map_err(|e| invalid(e.to_string()))?;
    if plan["kind"] != KIND || plan["schemaVersion"] != 1 {
        return Err(invalid("Invalid project plan"));
    }
    Ok(plan)
}
fn stage(
    tx: &mut OutputTransaction,
    policy: &OutputPolicy,
    root: &str,
    path: &str,
    bytes: &[u8],
) -> Result<(), DevupError> {
    tx.stage(
        path,
        policy.resolve(&Path::new(root).join(path).to_string_lossy())?,
        bytes,
    )
}
fn write(
    tx: &mut OutputTransaction,
    policy: &OutputPolicy,
    root: &str,
    plan: &Value,
) -> Result<(), DevupError> {
    let bytes = serde_json::to_vec_pretty(plan).map_err(|e| invalid(e.to_string()))?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(invalid("Project plan exceeds 2 MiB"));
    }
    stage(tx, policy, root, FILE, &bytes)
}
fn confirmed(plan: &Value) -> Result<(), DevupError> {
    if plan["state"] != "confirmed" {
        return Err(invalid(
            "Confirm project plan before implementation or split",
        ));
    }
    Ok(())
}
/// Saves decisions without dropping earlier ones; a decision with the same id
/// is replaced. Used by draft and confirm as well as defer/assume.
fn record_decisions(
    plan: &mut Value,
    decisions: &[Decision],
    state: &str,
) -> Result<(), DevupError> {
    for d in decisions {
        if d.id.trim().is_empty() || d.reason.trim().is_empty() || d.recommended.trim().is_empty()
        {
            return Err(invalid("Decisions need id, reason and recommended option"));
        }
        let list = plan["decisions"].as_array_mut().unwrap();
        list.retain(|v| v["id"] != d.id);
        let mut value = json!(d);
        value["state"] = json!(state);
        list.push(value);
    }
    Ok(())
}
fn figma_url(plan: &Value) -> String {
    let figma = &plan["facts"]["figma"];
    figma
        .as_str()
        .or_else(|| array(figma).first().and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned()
}
fn screen_blockers(plan: &Value, root: &str) -> Vec<String> {
    if figma_url(plan).is_empty() {
        return vec![];
    }
    match screen_plan::read_plan(root) {
        Ok(Some(s)) if s["state"] == "confirmed" || s["status"] == "confirmed" => vec![],
        _ => vec!["screenPlan: not confirmed".to_owned()],
    }
}
fn screen_plan_hint(plan: &Value, root: &str) -> String {
    format!(
        "Run {} first.",
        json!({"tool":"devup_screen_plan","arguments":{"action":"draft","url":figma_url(plan),"projectRoot":root}})
    )
}
fn next_action(plan: &Value, root: &str) -> Value {
    let args = |action: &str| json!({"action":action, "planId":plan["planId"], "projectRoot":root});
    if matches!(plan["state"].as_str(), Some("intake" | "draft"))
        && !screen_blockers(plan, root).is_empty()
    {
        return json!({
            "tool":"devup_screen_plan",
            "arguments":{"action":"draft","url":figma_url(plan),"projectRoot":root},
            "how":"Figma sources need a confirmed screen plan before project confirm; run it first and ask its questions together with this plan's."});
    }
    match plan["state"].as_str() {
        Some("confirmed") if !units::open_requests(plan).is_empty() => json!({
            "tool":"devup_project_plan","arguments":args("status"),
            "how":"Apply the open sharedChangeRequests to the stage 0 files, then send a unitUpdates entry of kind shared-change-request, state resolved and the file; merged units that depend on stage 0 go back to verifying."}),
        Some("confirmed") if array(&plan["units"]).is_empty() => json!({
            "tool":"devup_project_plan","arguments":args("split"),
            "how":"Models are written and unitDag is empty until split; run backendCommands in modelsWorkingDirectory first."}),
        Some("draft") => json!({
            "tool":"devup_project_plan","arguments":args("confirm"),
            "how":"Ask unanswered questions with the question tool, then confirm (acceptRecommended fills the rest)."}),
        _ => Value::Null,
    }
}
fn unit_rows(plan: &Value) -> Vec<Value> {
    array(&plan["units"])
        .iter()
        .map(|u| {
            json!({"id":u["id"],"layer":u["layer"],"state":u["state"],
                "dependencies":u["dependencies"],
                "brief":format!(".devup/units/{}.md", text(&u["id"]))})
        })
        .collect()
}
/// Questions of an unconfirmed screen plan, so one round covers both plans.
fn open_screen_plan(root: &str) -> Option<Value> {
    match screen_plan::read_plan(root) {
        Ok(Some(s)) if s["state"] != "confirmed" && s["status"] != "confirmed" => Some(s),
        _ => None,
    }
}
fn open_screen_questions(root: &str) -> Vec<Value> {
    open_screen_plan(root)
        .map(|s| array(&s["questions"]).to_vec())
        .unwrap_or_default()
}
/// Answers addressed to screen-plan questions are not project-plan answers:
/// they are split off and returned as the devup_screen_plan confirm call that
/// carries them, so the user answers one merged round.
fn forward_screen_answers(
    root: &str,
    answers: &BTreeMap<String, Value>,
    accept_recommended: bool,
) -> (BTreeMap<String, Value>, Value) {
    let Some(screen) = open_screen_plan(root) else {
        return (answers.clone(), Value::Null);
    };
    let ids = array(&screen["questions"])
        .iter()
        .map(|q| text(&q["id"]).to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    let (screen_answers, own): (BTreeMap<_, _>, BTreeMap<_, _>) = answers
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .partition(|(k, _)| ids.contains(k));
    if screen_answers.is_empty() && !(accept_recommended && !ids.is_empty()) {
        return (own, Value::Null);
    }
    let forward = json!({"tool":"devup_screen_plan","arguments":{
        "action":"confirm","planId":screen["planId"],"projectRoot":root,
        "answers":screen_answers,"acceptRecommended":accept_recommended},
        "how":"These answers belong to the screen plan: send them with this call. The user was asked once, in the merged proposal."});
    (own, forward)
}
fn without_screens(plan: &Value) -> Vec<Value> {
    array(&plan["features"])
        .iter()
        .filter(|f| array(&f["screens"]).is_empty())
        .map(|f| f["id"].clone())
        .collect()
}
fn change_summary(before: &Value, after: &Value) -> Value {
    let mut summary = json!({});
    for (field, key) in [("decisions", "id"), ("features", "id"), ("entities", "name"), ("answers", "")] {
        let records = |plan: &Value| -> BTreeMap<String, Value> {
            if key.is_empty() {
                plan[field].as_object().into_iter().flatten().map(|(k, v)| (k.clone(), v.clone())).collect()
            } else {
                array(&plan[field]).iter().map(|v| (text(&v[key]).to_owned(), v.clone())).collect()
            }
        };
        let old = records(before);
        let new = records(after);
        let mut changes = json!({"added":[],"changed":[],"kept":[],"removed":[]});
        for (id, value) in &new {
            let kind = match old.get(id) {
                None => "added",
                Some(previous) if previous == value => "kept",
                Some(_) => "changed",
            };
            changes[kind].as_array_mut().unwrap().push(json!(id));
        }
        changes["removed"] = json!(old.keys().filter(|id| !new.contains_key(*id)).collect::<Vec<_>>());
        summary[field] = changes;
    }
    summary
}
const DATA_UI: [&str; 9] = [
    "list", "detail", "form", "search", "inquiry", "filter", "pagination", "board", "table",
];
/// Set when the project plan has screens that read or write data but its ERD is
/// not confirmed yet; page writes are refused until then.
pub(crate) fn erd_required(root: &str) -> Option<Value> {
    let plan = read(root).ok()?;
    if plan["state"] == "confirmed" || !erd::api(&plan) {
        return None;
    }
    let data_ui = !array(&plan["entities"]).is_empty()
        || array(&plan["features"]).iter().any(|f| {
            let haystack = format!("{} {} {}", text(&f["scope"]), f["acceptance"], f["screens"])
                .to_lowercase();
            !array(&f["entities"]).is_empty() || DATA_UI.iter().any(|k| haystack.contains(k))
        });
    data_ui.then(|| json!({"planId":plan["planId"],"state":plan["state"]}))
}
fn blockers(plan: &Value, root: &str) -> Vec<String> {
    [screen_blockers(plan, root), wiring::blockers(plan, root)].concat()
}
fn extras(plan: &Value, root: &str) -> Value {
    let no_screens = without_screens(plan);
    let mut extra = json!({
        "writtenTo": plan["writtenTo"],
        "decisionCount": array(&plan["decisions"]).len(),
        "unitState": if array(&plan["units"]).is_empty() {"not-split"} else {"split"},
        "sharedChangeRequests": units::open_requests(plan),
        "apiWiring": wiring::report(plan, root),
        "backendChecks": wiring::backend_checks(plan, root),
    });
    if erd::api(plan) && array(&plan["entities"]).is_empty() {
        extra["erdDraftRequired"] = json!({
            "reason":"API scope has no entities; draft the ERD before approving it.",
            "nextAction":{"tool":"devup_project_plan","arguments":{
                "action":"draft","projectRoot":root,"planId":plan["planId"]},
                "how":"Supply entities and relations from the recorded requirements."}
        });
    }
    if !no_screens.is_empty() {
        extra["featuresWithoutScreens"] = json!({"features":no_screens,
            "how":"These features list no screens. Pass features[].screens (page or route names) in draft; no screens question is asked."});
    }
    extra
}
fn response(plan: Value, root: &str, full: bool) -> Value {
    let extra = extras(&plan, root);
    let mut out = base_response(plan, root, full);
    if let (Some(o), Some(e)) = (out.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    out
}
fn base_response(plan: Value, root: &str, full: bool) -> Value {
    let questions = array(&plan["questions"])
        .iter()
        .filter(|q| plan["answers"][text(&q["id"])].is_null())
        .cloned()
        .collect::<Vec<_>>();
    let screen_open = open_screen_questions(root);
    let screen_ids = screen_open.iter().map(|q| q["id"].clone()).collect::<Vec<_>>();
    let questions = [questions, screen_open].concat();
    let proposal = screen_plan::proposal(&questions);
    let instruction = if plan["state"] == "confirmed" {
        include_str!("../text/mod-response-22d8f704.md")
    } else {
        screen_plan::ASK_INSTRUCTION
    };
    if !full {
        return json!({"planId":plan["planId"], "state":plan["state"], "detail":"summary",
            "openQuestionCount":questions.len(),
            "openQuestionIds":questions.iter().map(|q| q["id"].clone()).collect::<Vec<_>>(),
            "screenPlanQuestionIds":screen_ids,
            "blockers":blockers(&plan, root),
            "proposalMarkdown":screen_plan::proposal_markdown(&proposal),
            "questionRounds":screen_plan::question_rounds(&questions, 4),
            "instruction":instruction,
            "nextAction":next_action(&plan, root),
            "erdMermaid":erd::mermaid(array(&plan["entities"])),
            "unitDag":units::dag(array(&plan["units"])),
            "units":unit_rows(&plan),
            "decisions":plan["decisions"],
            "backendCommands":plan["backendCommands"],
            "backendNotes":plan["backendNotes"],
            "modelsWorkingDirectory":plan["modelsWorkingDirectory"],
            "how":"Summary only. Pass detail \"full\" to read the whole plan."});
    }
    json!({"planId":plan["planId"], "state":plan["state"],
        "blockers":blockers(&plan, root),
        "proposalMarkdown":screen_plan::proposal_markdown(&proposal),
        "proposal":proposal,
        "screenPlanQuestionIds":screen_ids,
        "questions":screen_plan::ask_shape(&questions),
        "questionRounds":screen_plan::question_rounds(&questions, 4),
        "instruction":instruction,
        "nextAction":next_action(&plan, root),
        "erdMermaid":erd::mermaid(array(&plan["entities"])),
        "unitDag":units::dag(array(&plan["units"])),
        "finalReport":{"units":plan["units"], "decisions":plan["decisions"], "completion":"unverified until supervisor reruns all verification and stack diff reports no drift"},
        "plan":plan})
}

pub(super) async fn run(
    input: &ProjectPlanInput,
    policy: &OutputPolicy,
) -> Result<Value, DevupError> {
    if input.project_root.trim().is_empty() {
        return Err(invalid("projectRoot is required"));
    }
    if input
        .detail
        .as_deref()
        .is_some_and(|d| !["summary", "full"].contains(&d))
    {
        return Err(invalid("detail must be summary or full"));
    }
    let full = input.detail.as_deref() == Some("full");
    // Apply the same root boundary to reads and writes, even for status.
    policy.resolve(&Path::new(&input.project_root).join(FILE).to_string_lossy())?;
    let mut tx = OutputTransaction::new();
    if input.action == "intake" {
        if Path::new(&input.project_root).join(FILE).exists() {
            return Err(invalid(
                "Project plan exists; use draft/status instead of replacing decisions",
            ));
        }
        let facts = rounds::facts(input)?;
        let id = sha256(format!("{}{}", input.project_root, now()).as_bytes());
        let mut plan = json!({"kind":KIND,"schemaVersion":1,"planId":id,"state":"intake",
            "facts":facts,"evidence":input.evidence,"answers":{},"decisions":[],"units":[],
            "questions":rounds::intake(&facts),"entities":[],"relations":[],"features":[],"issues":[],
            "unitQuestions":"supervisor"});
        rounds::answers(&mut plan, &input.answers)?;
        write(&mut tx, policy, &input.project_root, &plan)?;
        tx.commit()?;
        return Ok(response(plan, &input.project_root, full));
    }
    let mut plan = read(&input.project_root)?;
    let previous = plan.clone();
    // status is a read: it may omit planId and reads it from the plan file.
    let provided = input.plan_id.is_some() || input.action != "status";
    if provided && input.plan_id.as_deref() != plan["planId"].as_str() {
        return Err(invalid(format!(
            "planId is missing or stale; current planId {} state {}",
            text(&plan["planId"]),
            text(&plan["state"])
        )));
    }
    let (answers, forward) = if matches!(input.action.as_str(), "draft" | "confirm") {
        let mut shared = BTreeMap::new();
        let fresh = rounds::facts(input)?;
        for key in ["siteName", "siteUrl", "favicon", "ogImage", "motion"] {
            let value = input.answers.get(key)
                .or_else(|| plan["answers"].get(key))
                .or_else(|| fresh.get(key))
                .or_else(|| plan["facts"].get(key));
            if let Some(value) = value.filter(|v| v.as_str().is_some_and(|s| !s.trim().is_empty()))
                && open_screen_questions(&input.project_root).iter().any(|q| q["id"] == key)
            {
                shared.insert(key.to_owned(), value.clone());
            }
        }
        shared.extend(input.answers.clone());
        forward_screen_answers(&input.project_root, &shared, input.accept_recommended)
    } else {
        (input.answers.clone(), Value::Null)
    };
    match input.action.as_str() {
        "draft" => {
            if plan["state"] == "confirmed" {
                return Err(invalid("Confirmed plans cannot be redrafted"));
            }
            let previous_answers = answers.iter()
                .filter(|(id, _)| array(&plan["questions"]).iter().any(|question| question["id"] == **id))
                .map(|(id, answer)| (id.clone(), answer.clone()))
                .collect();
            rounds::answers(&mut plan, &previous_answers)?;
            if input.accept_recommended {
                rounds::accept_recommended(&mut plan)?;
            }
            // Open questions no longer block draft: new evidence, features and
            // decisions are saved first and only confirm requires every answer.
            record_decisions(&mut plan, &input.decisions, "pending")?;
            for ev in &input.evidence {
                if ev.source.trim().is_empty() {
                    return Err(invalid("Evidence needs a source"));
                }
                for (key, value) in &ev.facts {
                    if !["parts", "goal", "workType"].contains(&key.as_str())
                        || plan["answers"][key].is_null()
                    {
                        plan["facts"][key] = value.clone();
                    }
                }
            }
            plan["evidence"]
                .as_array_mut()
                .unwrap()
                .extend(input.evidence.iter().map(|e| json!(e)));
            if !input.features.is_empty() {
                plan["features"] = json!(input.features);
            }
            if !input.issues.is_empty() {
                plan["issues"] = json!(input.issues);
            }
            if let Some(mode) = &input.unit_questions {
                if !["supervisor", "worker"].contains(&mode.as_str()) {
                    return Err(invalid("unitQuestions must be supervisor or worker"));
                }
                plan["unitQuestions"] = json!(mode);
            }
            erd::draft(input, &mut plan)?;
            plan["questions"] = json!(rounds::detail(&plan, &input.extra_questions)?);
            let new_answers = answers.into_iter()
                .filter(|(id, _)| !previous_answers.contains_key(id))
                .collect();
            rounds::answers(&mut plan, &new_answers)?;
            if input.accept_recommended {
                rounds::accept_recommended(&mut plan)?;
            }
            plan["state"] = json!("draft");
        }
        "confirm" => {
            if plan["state"] != "draft" {
                return Err(invalid("Draft the complete plan before confirm"));
            }
            rounds::answers(&mut plan, &answers)?;
            if input.accept_recommended {
                rounds::accept_recommended(&mut plan)?;
            }
            rounds::require_answers(&plan).map_err(|mut error| {
                error.details = json!({
                    "missingQuestionIds": array(&plan["questions"]).iter()
                        .filter(|q| plan["answers"][text(&q["id"])].is_null())
                        .map(|q| q["id"].clone()).collect::<Vec<_>>(),
                    "nextAction":{"tool":"devup_project_plan","arguments":{
                        "action":"confirm","projectRoot":input.project_root,
                        "planId":plan["planId"],"answers":{},"acceptRecommended":true}}
                });
                error
            })?;
            record_decisions(&mut plan, &input.decisions, "pending")?;
            if plan["answers"]
                .as_object()
                .unwrap()
                .values()
                .any(|v| v == "Revise ERD before confirming")
            {
                return Err(invalid(
                    "Redraft the structured ERD and approve it before confirm",
                ));
            }
            erd::draft(input, &mut plan)?;
            // Figma remains its own confirmation gate and is retained in every brief.
            if let Some(screen) = screen_plan::read_plan(&input.project_root)? {
                if screen["state"] != "confirmed" && screen["status"] != "confirmed" {
                    return Err(invalid(format!(
                        "Confirm the Figma screen plan first. {}",
                        screen_plan_hint(&plan, &input.project_root)
                    )));
                }
                plan["screenPlan"] = screen;
            } else if !figma_url(&plan).is_empty() {
                return Err(invalid(format!(
                    "Figma sources require a confirmed devup_screen_plan. {}",
                    screen_plan_hint(&plan, &input.project_root)
                )));
            }
            erd::write_models(&mut tx, policy, &input.project_root, &plan)?;
            plan["writtenTo"] = json!(if erd::api(&plan) {
                array(&plan["entities"]).iter()
                    .map(|entity| format!("{}/{}.json", text(&plan["modelsDir"]), text(&entity["name"])))
                    .collect::<Vec<_>>()
            } else { vec![] });
            plan["state"] = json!("confirmed");
            plan["confirmedAt"] = json!(now());
        }
        "split" => {
            confirmed(&plan)?;
            units::split(input, &mut plan, &mut tx, policy)?;
        }
        "defer" | "assume" => {
            confirmed(&plan)?;
            if input.action == "assume" && input.decisions.is_empty() {
                for d in plan["decisions"].as_array_mut().unwrap() {
                    if d["state"] == "pending" {
                        d["state"] = json!(if d["destructive"] == true {
                            "unbuilt"
                        } else {
                            "assumed"
                        });
                    }
                }
            }
            for d in &input.decisions {
                if d.id.trim().is_empty()
                    || d.reason.trim().is_empty()
                    || d.recommended.trim().is_empty()
                {
                    return Err(invalid("Decisions need id, reason and recommended option"));
                }
                let decisions = plan["decisions"].as_array_mut().unwrap();
                decisions.retain(|v| v["id"] != d.id);
                let mut value = json!(d);
                value["state"] = json!(if input.action == "defer" {
                    "pending"
                } else if d.destructive {
                    "unbuilt"
                } else {
                    "assumed"
                });
                decisions.push(value);
            }
        }
        "status" => {
            units::update(input, &mut plan)?;
        }
        _ => {
            return Err(invalid(
                "action must be intake, draft, confirm, status, assume, defer or split",
            ));
        }
    }
    wiring::record(&mut plan, &input.api_wiring)?;
    write(&mut tx, policy, &input.project_root, &plan)?;
    tx.commit()?;
    let changes = (input.action == "draft").then(|| change_summary(&previous, &plan));
    let mut out = response(plan, &input.project_root, full);
    if let Some(changes) = changes {
        out["changeSummary"] = changes;
    }
    if input.action == "status" && !full
        && let Some(summary) = out.as_object_mut()
    {
        summary.remove("questionRounds");
        summary.remove("proposalMarkdown");
    }
    if !forward.is_null() {
        out["screenPlanForward"] = forward;
    }
    Ok(out)
}
