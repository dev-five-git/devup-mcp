//! Project intake and supervisor state. Source reads are supplied by the runtime;
//! local evidence is collected before generating any unresolved questions.
mod erd;
mod rounds;
#[cfg(test)]
mod tests;
mod units;

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
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct UnitUpdate {
    pub id: String,
    pub state: String,
    #[serde(default)]
    pub verification: Vec<String>,
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
fn response(plan: Value) -> Value {
    let questions = array(&plan["questions"])
        .iter()
        .filter(|q| plan["answers"][text(&q["id"])].is_null())
        .cloned()
        .collect::<Vec<_>>();
    json!({"planId":plan["planId"], "state":plan["state"],
        "questions":screen_plan::ask_shape(&questions),
        "questionRounds":screen_plan::question_rounds(&questions, 4),
        "instruction": if plan["state"] == "confirmed" { include_str!("../text/mod-response-22d8f704.md") } else { screen_plan::ASK_INSTRUCTION },
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
        return Ok(response(plan));
    }
    let mut plan = read(&input.project_root)?;
    if input.plan_id.as_deref() != plan["planId"].as_str() {
        return Err(invalid("planId is missing or stale"));
    }
    match input.action.as_str() {
        "draft" => {
            if plan["state"] == "confirmed" {
                return Err(invalid("Confirmed plans cannot be redrafted"));
            }
            rounds::answers(&mut plan, &input.answers)?;
            rounds::require_answers(&plan)?;
            for ev in &input.evidence {
                if ev.source.trim().is_empty() {
                    return Err(invalid("Evidence needs a source"));
                }
                for (key, value) in &ev.facts {
                    plan["facts"][key] = value.clone();
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
            plan["state"] = json!("draft");
        }
        "confirm" => {
            if plan["state"] != "draft" {
                return Err(invalid("Draft the complete plan before confirm"));
            }
            rounds::answers(&mut plan, &input.answers)?;
            rounds::require_answers(&plan)?;
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
                    return Err(invalid("Confirm the Figma screen plan first"));
                }
                plan["screenPlan"] = screen;
            } else if !array(&plan["facts"]["figma"]).is_empty() {
                return Err(invalid(
                    "Figma sources require a confirmed devup_screen_plan",
                ));
            }
            erd::write_models(&mut tx, policy, &input.project_root, &plan)?;
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
    write(&mut tx, policy, &input.project_root, &plan)?;
    tx.commit()?;
    Ok(response(plan))
}
