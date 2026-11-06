//! Completion is a sequence of evidence-backed stages, never an export synonym.
use super::{
    artifacts::ArtifactStore,
    output::{OutputPolicy, OutputTransaction},
};
use devup_mcp_figma::{DevupError, ErrorCode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct CompletionInput {
    pub project_root: String,
    #[serde(default)]
    pub behavior_plan_paths: Vec<String>,
    #[serde(default)]
    pub audits: Vec<AuditPair>,
    #[serde(default)]
    pub check_receipt_path: Option<String>,
    /// Missing consumer-only renderer fields, keyed by the ingest comparison ID.
    /// Actual PNG, reference, threshold and observed capture context stay bound
    /// to the audit. Completion reruns comparisons; caller verdicts are not used.
    #[serde(default)]
    pub visual_environments: BTreeMap<String, super::visual_compare::VisualEnvironment>,
    #[serde(default)]
    pub data_plan_path: Option<String>,
    #[serde(default)]
    pub data_receipt_path: Option<String>,
    /// Explicit decisions for inferred presentation values that are static.
    /// Each value must explain the decision; a boolean blanket waiver is invalid.
    #[serde(default)]
    pub static_bindings: BTreeMap<String, String>,
    #[serde(default)]
    pub routes: Vec<String>,
    #[serde(default = "widths")]
    pub widths: Vec<u32>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct AuditPair {
    pub plan_path: String,
    pub report_path: String,
}
fn is_motion(candidate: &Value) -> bool {
    matches!(
        candidate["kind"].as_str(),
        Some(
            "count-up"
                | "marquee"
                | "progress-step-cycle"
                | "sliding-tab-indicator"
                | "section-entrance"
        )
    )
}

async fn verify_visual(
    comparison: &Value,
    environment: Option<&super::visual_compare::VisualEnvironment>,
    policy: &OutputPolicy,
    artifacts: &ArtifactStore,
) -> Value {
    let mut args = comparison["call"]["arguments"].clone();
    // A local file, including the implementation's own screenshot, cannot be
    // supplied as a replacement baseline to the completion gate.
    if args["reference"]["artifactId"].as_str().is_none_or(|id| id.starts_with('<'))
        || args["reference"].get("path").is_some()
        || comparison["blockers"].as_array().is_some_and(|v| !v.is_empty())
    {
        return json!({"id":comparison["id"],"verdict":"inconclusive","reason":"A valid Figma artifact and compatible capture are required."});
    }
    if let Some(environment) = environment {
        let observed = args["environment"].clone();
        args["environment"] = serde_json::to_value(environment).unwrap_or(Value::Null);
        if let Some(observed) = observed.as_object() {
            for (key, value) in observed {
                args["environment"][key] = value.clone();
            }
        }
    }
    let result = match serde_json::from_value::<super::tools::VisualCompareInput>(args) {
        Ok(input) => super::visual_compare::compare(input, policy, artifacts).await,
        Err(_) => return json!({"id":comparison["id"],"verdict":"inconclusive","reason":"Invalid visual comparison input."}),
    };
    match result {
        Ok(result) => {
            let reference_node = result["reference"]["nodeId"].as_str()
                .or_else(|| result["reference"]["target"]["nodeId"].as_str());
            let bound = comparison["screenshot"]["sha256"].as_str().is_some()
                && comparison["screenshot"]["sha256"] == result["actual"]["sha256"]
                && reference_node.is_some()
                && reference_node == comparison["nodeId"].as_str();
            json!({"id":comparison["id"],"routePath":comparison["routePath"],"viewport":comparison["viewport"],"class":comparison["class"],"evidenceBound":bound,"result":result,"verdict":if bound {result["verdict"].clone()} else {json!("inconclusive")}})
        },
        Err(error) => json!({"id":comparison["id"],"verdict":"inconclusive","reason":error.message}),
    }
}
fn complete_inventory_for(plan: &Value, record: &Value, scope: Option<&str>) -> bool {
    super::behavior_contracts::verified_coverage_inventory(plan).is_some()
        && plan["scopeNodeIds"]
            .as_array()
            .is_some_and(|scope| !scope.is_empty())
        && record["artifactId"]
            .as_str()
            .is_some_and(|id| !id.trim().is_empty() && plan["artifactId"] == id)
        && scope
            .filter(|id| !id.is_empty())
            .is_none_or(|id| values(plan, "scopeNodeIds").any(|scope| scope == id))
}
fn continuation(plan: &Value, complete_plans: &[Value]) -> Value {
    complete_plans
        .iter()
        .find(|complete| {
            complete["artifactId"] == plan["artifactId"]
                && complete["snapshotFingerprint"] == plan["snapshotFingerprint"]
                && complete["scopeNodeIds"] == plan["scopeNodeIds"]
        })
        .unwrap_or(plan)
        .clone()
}
async fn complete_behavior_plan(
    record: &Value,
    scope: Option<&str>,
    plan: &Value,
    complete_plans: &[Value],
    artifacts: &ArtifactStore,
) -> Option<Value> {
    let plan = continuation(plan, complete_plans);
    if complete_inventory_for(&plan, record, scope) {
        return Some(plan);
    }
    let artifact = artifacts.get(record["artifactId"].as_str()?).await?;
    let mut response = json!({"artifactId":artifact.artifact_id});
    if let Some(scope) = scope.filter(|id| !id.is_empty()) {
        if !artifact.payload.snapshot.nodes.contains_key(scope) {
            return None;
        }
        response["nodeId"] = json!(scope);
    } else if let Some(node) = &artifact.payload.target.node_id {
        response["nodeId"] = json!(node);
    }
    super::behavior_contracts::attach(&mut response, &artifact.payload.snapshot);
    let rebuilt = continuation(&response["behaviorPlan"], complete_plans);
    complete_inventory_for(&rebuilt, record, scope).then_some(rebuilt)
}
fn planned_motion_coverage(plan: &Value, report: &Value) -> BTreeSet<String> {
    let mut covered = BTreeSet::new();
    for viewport in values(&plan["plan"], "viewports") {
        let runs = values(report, "runs")
            .filter(|run| {
                run["viewport"]["width"] == viewport["width"]
                    && run["viewport"]["height"] == viewport["height"]
                    && run["viewport"]["deviceScaleFactor"].as_f64().unwrap_or(1.0)
                        == viewport["deviceScaleFactor"].as_f64().unwrap_or(1.0)
            })
            .collect::<Vec<_>>();
        if runs.len() != 1 {
            continue;
        }
        for screen in values(&plan["plan"], "screens") {
            let screens = values(runs[0], "screens")
                .filter(|row| row["id"] == screen["id"] && row["status"] == "completed")
                .collect::<Vec<_>>();
            if screens.len() != 1 {
                continue;
            }
            for (index, check) in values(screen, "checks").enumerate().filter(|(_, check)| {
                check["kind"] == "motion"
                    && check["unavailableReason"].is_null()
                    && check["widths"].as_array().is_none_or(|widths| {
                        widths.is_empty() || widths.contains(&viewport["width"])
                    })
            }) {
                let rows = values(screens[0], "checks")
                    .filter(|row| row["index"].as_u64() == Some(index as u64))
                    .collect::<Vec<_>>();
                if rows.len() == 1
                    && rows[0]["kind"] == "motion"
                    && rows[0]["status"] == "pass"
                    && rows[0]["measured"]["candidateId"] == check["motion"]["candidateId"]
                    && let Some(id) = check["motion"]["candidateId"].as_str()
                {
                    covered.insert(id.to_owned());
                }
            }
        }
    }
    covered
}
fn widths() -> Vec<u32> {
    vec![360, 390, 858, 1100, 1440]
}
fn invalid(message: impl Into<String>) -> DevupError {
    DevupError::new(ErrorCode::DevupInvalidInput, message, false)
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(super) fn source_digest(
    root: &Path,
    policy: &OutputPolicy,
) -> Result<(String, BTreeMap<String, String>), DevupError> {
    const SKIP: &[&str] = &[
        ".git",
        ".devup",
        "node_modules",
        "target",
        "dist",
        "build",
        ".next",
        "df",
        "out",
        ".playwright",
        "playwright-report",
        "test-results",
        "coverage",
        ".cache",
        ".agents",
        ".codex",
        ".claude",
        ".opencode",
        ".venv",
        "__pycache__",
    ];
    const EXT: &[&str] = &[
        "ts", "tsx", "js", "jsx", "mjs", "cjs", "json", "rs", "toml", "lock", "html", "svg", "png",
        "jpg", "jpeg", "webp", "gif", "pdf", "woff", "woff2", "otf", "ttf", "sql", "prisma",
        "graphql", "yaml", "yml", "md", "mdx", "css", "scss", "avif", "ico", "webm", "mp4",
    ];
    fn visit(
        root: &Path,
        dir: &Path,
        policy: &OutputPolicy,
        files: &mut BTreeMap<String, String>,
        total: &mut usize,
    ) -> Result<(), DevupError> {
        for entry in
            std::fs::read_dir(dir).map_err(|_| invalid("Cannot enumerate source inputs."))?
        {
            let entry = entry.map_err(|_| invalid("Cannot enumerate a source entry."))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                return Err(invalid("Source names must be UTF-8."));
            };
            if SKIP.contains(&name) || name.starts_with(".env") || name.ends_with(".log") {
                continue;
            }
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path)
                .map_err(|_| invalid("Cannot inspect source input."))?;
            if meta.file_type().is_symlink() {
                return Err(invalid("Source digest refuses symbolic links."));
            }
            if meta.is_dir() {
                visit(root, &path, policy, files, total)?;
            } else if meta.is_file()
                && (path.strip_prefix(root).is_ok_and(|relative| {
                    relative
                        .components()
                        .any(|part| part.as_os_str() == "public")
                }) || path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|e| EXT.contains(&e.to_ascii_lowercase().as_str())))
            {
                if files.len() >= 10000
                    || meta.len() > 16 * 1024 * 1024
                    || (*total as u64) + meta.len() > 128 * 1024 * 1024
                {
                    return Err(invalid(
                        "Source digest bounds exceeded; no partial project digest was produced.",
                    ));
                }
                let bytes = policy
                    .resolve(&path.to_string_lossy())?
                    .read_bounded(16 * 1024 * 1024)?;
                *total += bytes.len();
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| invalid("Source path escaped project root."))?
                    .to_string_lossy()
                    .replace('\\', "/");
                files.insert(relative, hash(&bytes));
            }
        }
        Ok(())
    }
    let root = dunce::canonicalize(root).map_err(|_| invalid("projectRoot must exist."))?;
    let mut files = BTreeMap::new();
    visit(&root, &root, policy, &mut files, &mut 0)?;
    Ok((
        hash(&serde_json::to_vec(&files).map_err(|_| invalid("Cannot encode source digest."))?),
        files,
    ))
}

fn read(policy: &OutputPolicy, path: &str) -> Result<Value, DevupError> {
    serde_json::from_slice(&policy.resolve(path)?.read_bounded(8 * 1024 * 1024)?)
        .map_err(|_| invalid("Evidence is not bounded valid JSON."))
}
fn values<'a>(v: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    v[key].as_array().into_iter().flatten()
}

fn export_records(root: &Path, policy: &OutputPolicy) -> Result<Vec<Value>, DevupError> {
    let directory = root.join(".devup/exports");
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in
        std::fs::read_dir(directory).map_err(|_| invalid("Cannot enumerate export evidence."))?
    {
        let path = entry
            .map_err(|_| invalid("Cannot inspect export evidence."))?
            .path();
        if path.extension().and_then(|v| v.to_str()) != Some("json") {
            continue;
        }
        if records.len() >= 1000 {
            return Err(invalid(
                "Export evidence inventory exceeds its limit; no partial completion was produced.",
            ));
        }
        let record = read(policy, &path.to_string_lossy())?;
        if record["kind"] == "devup-export-record" {
            records.push(record);
        }
    }
    Ok(records)
}

pub(super) async fn evaluate(
    input: &CompletionInput,
    policy: &OutputPolicy,
    artifacts: &ArtifactStore,
) -> Result<Value, DevupError> {
    if input.widths.is_empty()
        || input.widths.len() > 8
        || input.widths.iter().any(|w| *w == 0 || *w > 4096)
        || input.audits.len() > 60
        || input.visual_environments.len() > 1000
        || input.behavior_plan_paths.len() > 1000
        || input.routes.len() > 1000
        || input.static_bindings.len() > 10000
    {
        return Err(invalid(
            "Completion requires one to eight bounded widths, at most sixty audits, one thousand behavior plans/routes and ten thousand static binding decisions.",
        ));
    }
    if input
        .static_bindings
        .values()
        .any(|why| why.trim().len() < 12 || why.len() > 2000)
    {
        return Err(invalid(
            "Static binding decisions require a concrete explanation (12..2000 bytes).",
        ));
    }
    let root =
        dunce::canonicalize(&input.project_root).map_err(|_| invalid("projectRoot must exist."))?;
    let (source_digest, files) = source_digest(&root, policy)?;
    let records = export_records(&root, policy)?;
    let mut required_bindings = BTreeSet::new();
    let mut required_motion = BTreeSet::new();
    let mut truncated = false;
    let mut complete_plans = Vec::new();
    for path in &input.behavior_plan_paths {
        let value = read(policy, path)?;
        let plan = if value["behaviorPlan"].is_object() {
            value["behaviorPlan"].clone()
        } else {
            value
        };
        if super::behavior_contracts::verified_coverage_inventory(&plan).is_none() {
            return Err(invalid(
                "Behavior continuation evidence is incomplete or its signed inventory does not match.",
            ));
        }
        complete_plans.push(plan);
    }
    for record in &records {
        let mut plans = vec![(
            record["rootNodeId"].as_str(),
            record["behaviorPlan"].clone(),
        )];
        plans.extend(
            values(record, "frames")
                .map(|frame| (frame["nodeId"].as_str(), frame["behaviorPlan"].clone())),
        );
        for (scope, plan) in plans {
            let Some(plan) =
                complete_behavior_plan(record, scope, &plan, &complete_plans, artifacts).await
            else {
                truncated = true;
                continue;
            };
            let Some((candidates, bindings)) =
                super::behavior_contracts::verified_coverage_inventory(&plan)
            else {
                truncated = true;
                continue;
            };
            required_bindings.extend(
                bindings
                    .into_iter()
                    .filter(|id| !input.static_bindings.contains_key(id)),
            );
            for candidate in candidates {
                if is_motion(&candidate)
                    && let Some(id) = candidate["id"].as_str()
                {
                    required_motion.insert(id.to_owned());
                }
            }
        }
    }
    let generated = !records.is_empty() && !files.is_empty() && !truncated;
    let checks = input
        .check_receipt_path
        .as_deref()
        .map(|p| read(policy, p))
        .transpose()?;
    let checked = checks.as_ref().is_some_and(|r| {
        r["kind"] == "devup-project-check-receipt"
            && r["sourceDigest"] == source_digest
            && r["sourceChanged"] == false
            && r["schemaVersion"] == 1
    });
    let mut missing_checks = Vec::new();
    for required in ["build", "lint", "typecheck", "test", "e2e"] {
        let passing = checked
            && checks.as_ref().is_some_and(|r| {
                values(r, "checks").any(|c| {
                    c["name"] == required
                        && c["exitCode"] == 0
                        && c["command"].as_array().is_some_and(|v| !v.is_empty())
                })
            });
        if !passing {
            missing_checks.push(required);
        }
    }
    let implemented = generated && missing_checks.is_empty();
    let mut routes = input.routes.iter().cloned().collect::<BTreeSet<_>>();
    let plan_file = root.join(".devup/screen-plan.json");
    if plan_file.is_file() {
        let plan = read(policy, &plan_file.to_string_lossy())?;
        routes.extend(values(&plan, "routes").filter_map(|r| r["url"].as_str().map(str::to_owned)));
    }
    let mut covered = BTreeSet::new();
    let mut required_visual = BTreeSet::new();
    let mut visual_covered = BTreeSet::new();
    let mut visual_results = Vec::new();
    let mut motion_covered = BTreeSet::new();
    let mut audit_results = Vec::new();
    let mut audit_ok = !input.audits.is_empty();
    for pair in &input.audits {
        let plan = read(policy, &pair.plan_path)?;
        let raw = read(policy, &pair.report_path)?;
        let ingested = super::runtime_audit::ingest_with_artifacts(
            policy,
            &pair.plan_path,
            &pair.report_path,
            artifacts,
        )
        .await?;
        let observed = &raw["sourceVerification"];
        let bound = plan["plan"]["sourceDigest"] == source_digest
            && raw["sourceDigest"] == source_digest
            && observed["beforeDigest"] == source_digest
            && observed["afterDigest"] == source_digest
            && observed["unchanged"] == true
            && observed["planMatched"] == true;
        let pass = bound
            && ingested["verdict"] == "pass"
            && ingested["integrity"]["planFileHashMatches"] == true;
        audit_ok &= pass;
        if pass {
            for comparison in values(&ingested, "visualComparisons") {
                let result = verify_visual(comparison,
                    comparison["id"].as_str().and_then(|id| input.visual_environments.get(id)),
                    policy, artifacts).await;
                let visual_pass = result["verdict"] == "pass";
                audit_ok &= visual_pass;
                if visual_pass && comparison["class"] == "page-visual"
                    && let (Some(route), Some(width)) = (comparison["routePath"].as_str(), comparison["viewport"]["width"].as_u64())
                {
                    visual_covered.insert((route.to_owned(), width as u32));
                }
                visual_results.push(result);
            }
            for screen in values(&plan["plan"], "screens") {
                if let Some(route) = screen["routePath"].as_str() {
                    for viewport in values(&plan["plan"], "viewports") {
                        if let Some(width) = viewport["width"].as_u64() {
                            let drawn = screen["figma"]["drawnWidths"].as_array();
                            if drawn.is_none_or(|v| v.is_empty() || v.iter().any(|w| w.as_u64() == Some(width))) {
                                required_visual.insert((route.to_owned(), width as u32));
                            }
                            let kinds = values(screen, "checks")
                                .filter(|check| {
                                    check["widths"].as_array().is_none_or(|v| {
                                        v.is_empty()
                                            || v.iter().any(|value| value.as_u64() == Some(width))
                                    })
                                })
                                .filter_map(|check| check["kind"].as_str())
                                .collect::<BTreeSet<_>>();
                            if [
                                "page-health",
                                "no-horizontal-overflow",
                                "no-clipped-content",
                                "markup",
                                "rendered-completeness",
                            ]
                            .iter()
                            .all(|kind| kinds.contains(kind))
                            {
                                covered.insert((route.to_owned(), width as u32));
                            }
                        }
                    }
                }
            }
            motion_covered.extend(planned_motion_coverage(&plan, &raw));
        }
        audit_results.push(json!({"planPath":pair.plan_path,"reportPath":pair.report_path,"sourceBound":bound,"verdict":ingested["verdict"],"pass":pass}));
    }
    let missing_routes = routes
        .iter()
        .flat_map(|r| input.widths.iter().map(move |w| (r.clone(), *w)))
        .filter(|pair| !covered.contains(pair))
        .collect::<Vec<_>>();
    let missing_motion = required_motion
        .difference(&motion_covered)
        .cloned()
        .collect::<Vec<_>>();
    let missing_visual = required_visual.difference(&visual_covered).cloned().collect::<Vec<_>>();
    let browser_verified = implemented
        && audit_ok
        && !routes.is_empty()
        && missing_routes.is_empty()
        && missing_motion.is_empty()
        && !required_visual.is_empty()
        && missing_visual.is_empty();
    let mut data = json!({"sourceMatched":false,"apiVerified":false,"dbVerified":false,"coveredBindingIds":[]});
    if let (Some(plan_path), Some(receipt_path)) = (&input.data_plan_path, &input.data_receipt_path)
    {
        data = match super::data_verification::verify_data_evidence(
            policy,
            receipt_path,
            &source_digest,
            plan_path,
        ) {
            Ok(value) => value,
            Err(error) => {
                json!({"verified":false,"sourceMatched":false,"apiVerified":false,"dbVerified":false,"coveredBindingIds":[],"reason":error.message,"errorCode":error.code})
            }
        };
        let statuses = values(&data, "bindings").cloned().collect::<Vec<_>>();
        for key in ["sourceMatched", "apiVerified", "dbVerified"] {
            data[key] = json!(
                !statuses.is_empty()
                    && data["verified"] == true
                    && statuses.iter().all(|binding| binding[key] == true)
            );
        }
    }
    let binding_ids = values(&data, "coveredBindingIds")
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let missing_bindings = required_bindings
        .difference(&binding_ids)
        .cloned()
        .collect::<Vec<_>>();
    let data_verified = browser_verified
        && (required_bindings.is_empty()
            || data["sourceMatched"] == true
                && data["apiVerified"] == true
                && data["dbVerified"] == true
                && missing_bindings.is_empty());
    let stage = if data_verified {
        "data-verified"
    } else if browser_verified {
        "browser-verified"
    } else if implemented {
        "implemented"
    } else if generated {
        "generated"
    } else {
        "incomplete"
    };
    Ok(
        json!({"kind":"devup-completion-receipt","schemaVersion":1,"sourceDigest":source_digest,"sourceFileCount":files.len(),"sourceScope":"project-source-and-public-assets","stage":stage,"done":data_verified,"reportAllowed":data_verified,
        "stages":{"generated":generated,"implemented":implemented,"browserVerified":browser_verified,"dataVerified":data_verified},
        "requiredMotionIds":required_motion,"requiredBindingIds":required_bindings,"staticBindingDecisions":input.static_bindings,
        "blockers":{"incompleteBehaviorCoverage":truncated,"missingChecks":missing_checks,"missingRoutesOrWidths":missing_routes,"missingVisualComparisons":missing_visual,"missingMotion":missing_motion,"missingBindings":missing_bindings},
        "audits":audit_results,"visualResults":visual_results,"dataEvidence":data,"dataVerificationScope":if required_bindings.is_empty(){"not-required"}else{"isolated-database-snapshots"},"input":input,
        "trust":"Command exits, browser measurements and SQL rows are executor-observed; source digests, plan/report bindings and referenced artifact hashes are independently rechecked. A generated export or a caller success boolean never establishes application completion."}),
    )
}

pub(super) async fn write(
    input: &CompletionInput,
    policy: &OutputPolicy,
    artifacts: &ArtifactStore,
    output_path: Option<&str>,
) -> Result<Value, DevupError> {
    let mut result = evaluate(input, policy, artifacts).await?;
    let default = Path::new(&input.project_root).join(".devup/completion.json");
    let canonical = policy.resolve(&default.to_string_lossy())?;
    result["receiptPath"] = json!(canonical.display_path().to_string_lossy());
    let bytes =
        serde_json::to_vec_pretty(&result).map_err(|_| invalid("Cannot encode completion."))?;
    let mut transaction = OutputTransaction::new();
    if let Some(path) = output_path {
        let target = policy.resolve(path)?;
        if target.display_path() != canonical.display_path() {
            transaction.stage("completion-copy", target, &bytes)?;
        }
    }
    transaction.stage("completion", canonical, &bytes)?;
    transaction.commit()?;
    Ok(result)
}

pub(super) async fn refresh(
    root: &str,
    policy: &OutputPolicy,
    artifacts: &ArtifactStore,
) -> Result<Value, DevupError> {
    let path = Path::new(root).join(".devup/completion.json");
    if !path.is_file() {
        return Ok(
            json!({"stage":"unverified","done":false,"reportAllowed":false,"reason":"Run devup_runtime_audit completion with bound build, browser and data evidence."}),
        );
    }
    let saved = read(policy, &path.to_string_lossy())?;
    let input: CompletionInput = serde_json::from_value(saved["input"].clone())
        .map_err(|_| invalid("Completion receipt has no valid inputs."))?;
    if dunce::canonicalize(&input.project_root).ok() != dunce::canonicalize(root).ok() {
        return Err(invalid("Completion receipt belongs to another project."));
    }
    evaluate(&input, policy, artifacts).await
}

#[cfg(test)]
#[path = "completion_tests.rs"]
mod tests;
