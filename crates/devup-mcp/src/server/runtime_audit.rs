//! `devup_runtime_audit`: a declarative plan for a browser run the consumer's
//! project executes, and the ingestion of the report that run writes.
//!
//! The server never launches a browser or runs a command. `plan` validates a
//! schema-checked plan - routes, viewports, steps by role and accessible name
//! or test id, geometry checks - and gives it an id derived from its content.
//! The consumer-side executor (`harness/runtime-audit`) runs it in fresh,
//! isolated browser contexts and writes a report with screenshots. `ingest`
//! reads both from the allowed roots and trusts neither blindly: it rebinds the
//! report to the plan by id and by the plan file's hash, recomputes every count
//! from the items, verifies each screenshot's hash, size and decoded width, and
//! says which facts it verified and which only the executor declared.

use std::collections::BTreeMap;
use std::path::Path;

use devup_mcp_figma::{DevupError, ErrorCode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::output::{OutputPolicy, OutputTransaction};

pub(crate) const PLAN_KIND: &str = "devup-runtime-audit-plan";
pub(crate) const REPORT_KIND: &str = "devup-runtime-audit-report";
const SCHEMA_VERSION: u64 = 1;
const MAX_VIEWPORTS: usize = 8;
const MAX_SCREENS: usize = 60;
const MAX_STEPS: usize = 20;
const MAX_CHECKS: usize = 40;
const MAX_TIMEOUT_MS: u64 = 120_000;
const MAX_VIEWPORT_SIDE: u32 = 4096;
const MAX_TEXT: usize = 300;
const MAX_PLAN_BYTES: usize = 1024 * 1024;
const MAX_REPORT_BYTES: usize = 8 * 1024 * 1024;
const MAX_SCREENSHOT_BYTES: usize = 16 * 1024 * 1024;
const MAX_SCREENSHOTS: usize = 400;
const MAX_LISTED: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct RuntimeAuditInput {
    /// plan: validate and write a plan for the consumer-side executor.
    /// ingest: verify the executor's report against its plan.
    #[schemars(extend("enum" = ["plan", "ingest"]))]
    pub action: String,
    /// Required for plan.
    #[serde(default)]
    pub plan: Option<PlanInput>,
    /// plan: where to write the plan JSON, under an allowed write root.
    #[serde(default)]
    pub output_path: Option<String>,
    /// ingest: the plan JSON the executor ran.
    #[serde(default)]
    pub plan_path: Option<String>,
    /// ingest: the executor's report.json. Screenshot paths in it are
    /// relative to its directory.
    #[serde(default)]
    pub report_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct PlanInput {
    /// http(s) URL of the running consumer app, e.g. http://127.0.0.1:4173.
    pub base_url: String,
    /// Origins the run may load from; every other request is blocked and
    /// recorded. Must include the baseUrl origin.
    pub allowed_origins: Vec<String>,
    pub viewports: Vec<ViewportInput>,
    /// Visited in order in one isolated browser context per viewport.
    pub screens: Vec<ScreenInput>,
    #[serde(default)]
    pub readiness: Option<ReadinessInput>,
    /// Font families that must be loaded before checks; a missing one is an
    /// environment error, not a layout failure.
    #[serde(default)]
    pub fonts: Vec<String>,
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub step_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct ViewportInput {
    pub width: u32,
    pub height: u32,
    #[serde(default = "default_scale")]
    pub device_scale_factor: f64,
    #[serde(default)]
    pub mobile: bool,
    #[serde(default)]
    pub touch: bool,
}

fn default_scale() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct ScreenInput {
    pub id: String,
    /// Navigated to directly. Omitted, the screen is reached by its steps
    /// from the previous one.
    #[serde(default)]
    pub route_path: Option<String>,
    /// The component this screen renders, recorded as an anchor only.
    #[serde(default)]
    pub component_path: Option<String>,
    #[serde(default)]
    pub steps: Vec<StepInput>,
    #[serde(default)]
    pub expect: Option<ExpectInput>,
    #[serde(default)]
    pub checks: Vec<CheckInput>,
    /// The Figma frame this screen was generated from and the widths it was
    /// drawn at. Only those widths have a visual original.
    #[serde(default)]
    pub figma: Option<FigmaAnchorInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct StepInput {
    #[schemars(extend("enum" = ["click", "fill"]))]
    pub action: String,
    pub target: LocatorInput,
    /// The text for fill.
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// Exactly one way of finding an element: a role with its accessible name, a
/// test id, visible text, or a CSS selector (checks only).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct LocatorInput {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub test_id: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub css: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct ExpectInput {
    /// The path the page must reach, compared without origin or query.
    #[serde(default)]
    pub url_path: Option<String>,
    /// A heading with this accessible name must be visible.
    #[serde(default)]
    pub heading: Option<String>,
    #[serde(default)]
    pub visible: Option<LocatorInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct CheckInput {
    /// no-horizontal-overflow: the page is no wider than the viewport.
    /// contained: subject's box lies inside container's.
    /// aligned: subject keeps offsetPx from container's edge (left/right) or
    /// shares its centre (center).
    /// spans: subject spans container's width less insetPx on each side.
    /// visible: subject renders with a non-empty box.
    #[schemars(extend("enum" = ["no-horizontal-overflow", "contained", "aligned", "spans", "visible"]))]
    pub kind: String,
    #[serde(default)]
    pub subject: Option<LocatorInput>,
    #[serde(default)]
    pub container: Option<LocatorInput>,
    #[serde(default)]
    #[schemars(extend("enum" = ["left", "right", "center"]))]
    pub edge: Option<String>,
    #[serde(default)]
    pub offset_px: Option<f64>,
    #[serde(default)]
    pub inset_px: Option<f64>,
    #[serde(default)]
    pub tolerance_px: Option<f64>,
    /// What of the subject is measured: its element box (default), or the
    /// box its CSS mask paints - a rule drawn as a mask can keep its drawn
    /// length inside an element box that stretches.
    #[serde(default)]
    #[schemars(extend("enum" = ["box", "painted-mask"]))]
    pub measure: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct ReadinessInput {
    /// A selector the app renders once it is ready to be measured.
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct FigmaAnchorInput {
    pub node_id: String,
    pub drawn_widths: Vec<u32>,
}

fn invalid(message: impl Into<String>) -> DevupError {
    DevupError::new(ErrorCode::DevupInvalidInput, message, false)
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn origin_of(url: &str) -> Result<String, DevupError> {
    let parsed = url::Url::parse(url).map_err(|_| invalid(format!("{url} is not a URL.")))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(invalid(format!(
            "{url} must be an http or https URL with a host."
        )));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(invalid(
            "URLs in a runtime audit plan cannot carry credentials.",
        ));
    }
    Ok(parsed.origin().ascii_serialization())
}

fn bounded_text(field: &str, value: &str) -> Result<(), DevupError> {
    if value.trim().is_empty() || value.chars().count() > MAX_TEXT {
        return Err(invalid(format!(
            "{field} must be non-empty and at most {MAX_TEXT} characters."
        )));
    }
    Ok(())
}

fn timeout(field: &str, value: Option<u64>) -> Result<(), DevupError> {
    if value.is_some_and(|ms| ms == 0 || ms > MAX_TIMEOUT_MS) {
        return Err(invalid(format!(
            "{field} must be between 1 and {MAX_TIMEOUT_MS} ms."
        )));
    }
    Ok(())
}

fn validate_locator(
    field: &str,
    locator: &LocatorInput,
    allow_css: bool,
) -> Result<(), DevupError> {
    let ways = [
        locator.role.is_some(),
        locator.test_id.is_some(),
        locator.text.is_some(),
        locator.css.is_some(),
    ]
    .iter()
    .filter(|set| **set)
    .count();
    if ways != 1 {
        return Err(invalid(format!(
            "{field} needs exactly one of role (with name), testId, text{}.",
            if allow_css { " or css" } else { "" }
        )));
    }
    if locator.css.is_some() && !allow_css {
        return Err(invalid(format!(
            "{field} must be found by role and accessible name, testId or text; css is for checks only."
        )));
    }
    if locator.name.is_some() && locator.role.is_none() {
        return Err(invalid(format!("{field}.name needs role.")));
    }
    for (name, value) in [
        ("role", &locator.role),
        ("name", &locator.name),
        ("testId", &locator.test_id),
        ("text", &locator.text),
        ("css", &locator.css),
    ] {
        if let Some(value) = value {
            bounded_text(&format!("{field}.{name}"), value)?;
        }
    }
    Ok(())
}

fn validate_route(field: &str, path: &str) -> Result<(), DevupError> {
    bounded_text(field, path)?;
    if !path.starts_with('/') || path.starts_with("//") || path.contains('\\') {
        return Err(invalid(format!(
            "{field} must be a path on the baseUrl origin, starting with a single /."
        )));
    }
    Ok(())
}

/// Every rule a plan must satisfy before an executor may run it.
pub(crate) fn validate_plan(plan: &PlanInput) -> Result<(), DevupError> {
    let base_origin = origin_of(&plan.base_url)?;
    let allowed = plan
        .allowed_origins
        .iter()
        .map(|origin| origin_of(origin))
        .collect::<Result<Vec<_>, _>>()?;
    if !allowed.contains(&base_origin) {
        return Err(invalid(format!(
            "allowedOrigins must include the baseUrl origin {base_origin}."
        )));
    }
    if plan.viewports.is_empty() || plan.viewports.len() > MAX_VIEWPORTS {
        return Err(invalid(format!(
            "viewports must hold 1 to {MAX_VIEWPORTS} entries."
        )));
    }
    for viewport in &plan.viewports {
        if !(1..=MAX_VIEWPORT_SIDE).contains(&viewport.width)
            || !(1..=MAX_VIEWPORT_SIDE).contains(&viewport.height)
            || !(viewport.device_scale_factor > 0.0 && viewport.device_scale_factor <= 4.0)
        {
            return Err(invalid(format!(
                "Each viewport needs width and height in 1..={MAX_VIEWPORT_SIDE} and a deviceScaleFactor in (0, 4]."
            )));
        }
    }
    if plan.screens.is_empty() || plan.screens.len() > MAX_SCREENS {
        return Err(invalid(format!(
            "screens must hold 1 to {MAX_SCREENS} entries."
        )));
    }
    if plan.viewports.len() * plan.screens.len() > MAX_SCREENSHOTS {
        return Err(invalid(format!(
            "viewports x screens must be at most {MAX_SCREENSHOTS}, the screenshots ingest verifies for one report; split the plan."
        )));
    }
    if plan.screens[0].route_path.is_none() {
        return Err(invalid("The first screen needs a routePath to start from."));
    }
    let mut ids = std::collections::BTreeSet::new();
    for screen in &plan.screens {
        bounded_text("screens[].id", &screen.id)?;
        if !ids.insert(screen.id.as_str()) {
            return Err(invalid(format!("Screen id {} is repeated.", screen.id)));
        }
        if let Some(path) = &screen.route_path {
            validate_route("screens[].routePath", path)?;
        }
        if let Some(path) = &screen.component_path {
            bounded_text("screens[].componentPath", path)?;
        }
        if screen.route_path.is_none() && screen.steps.is_empty() {
            return Err(invalid(format!(
                "Screen {} needs a routePath or the steps that reach it.",
                screen.id
            )));
        }
        if screen.steps.len() > MAX_STEPS || screen.checks.len() > MAX_CHECKS {
            return Err(invalid(format!(
                "Screen {} exceeds {MAX_STEPS} steps or {MAX_CHECKS} checks.",
                screen.id
            )));
        }
        for step in &screen.steps {
            validate_locator("steps[].target", &step.target, false)?;
            timeout("steps[].timeoutMs", step.timeout_ms)?;
            match (step.action.as_str(), &step.value) {
                ("click", None) => {}
                ("fill", Some(value)) if value.chars().count() <= MAX_TEXT => {}
                _ => {
                    return Err(invalid(
                        "A step is click (no value) or fill (with a value of at most 300 characters).",
                    ));
                }
            }
        }
        if let Some(expect) = &screen.expect {
            if let Some(path) = &expect.url_path {
                validate_route("expect.urlPath", path)?;
            }
            if let Some(heading) = &expect.heading {
                bounded_text("expect.heading", heading)?;
            }
            if let Some(visible) = &expect.visible {
                validate_locator("expect.visible", visible, true)?;
            }
        }
        for check in &screen.checks {
            validate_check(check)?;
        }
        if let Some(figma) = &screen.figma {
            bounded_text("figma.nodeId", &figma.node_id)?;
            if figma.drawn_widths.is_empty() {
                return Err(invalid("figma.drawnWidths must name at least one width."));
            }
        }
    }
    for font in &plan.fonts {
        bounded_text("fonts[]", font)?;
    }
    if let Some(readiness) = &plan.readiness {
        if let Some(selector) = &readiness.selector {
            bounded_text("readiness.selector", selector)?;
        }
        timeout("readiness.timeoutMs", readiness.timeout_ms)?;
    }
    timeout("stepTimeoutMs", plan.step_timeout_ms)
}

fn validate_check(check: &CheckInput) -> Result<(), DevupError> {
    let needs = |subject: bool, container: bool| -> Result<(), DevupError> {
        if subject != check.subject.is_some() || container != check.container.is_some() {
            return Err(invalid(format!(
                "A {} check takes {}subject and {}container.",
                check.kind,
                if subject { "a " } else { "no " },
                if container { "a " } else { "no " },
            )));
        }
        Ok(())
    };
    match check.kind.as_str() {
        "no-horizontal-overflow" => needs(false, false)?,
        "visible" => needs(true, false)?,
        "contained" | "spans" => needs(true, true)?,
        "aligned" => {
            needs(true, true)?;
            if !matches!(check.edge.as_deref(), Some("left" | "right" | "center")) {
                return Err(invalid(
                    "An aligned check needs edge left, right or center.",
                ));
            }
        }
        other => return Err(invalid(format!("Unknown check kind {other}."))),
    }
    for (field, locator) in [("subject", &check.subject), ("container", &check.container)] {
        if let Some(locator) = locator {
            validate_locator(&format!("checks[].{field}"), locator, true)?;
        }
    }
    match check.measure.as_deref() {
        None | Some("box") => {}
        Some("painted-mask") if check.subject.is_some() => {}
        Some(other) => {
            return Err(invalid(format!(
                "measure {other} is not box, or painted-mask on a check with a subject."
            )));
        }
    }
    for value in [check.offset_px, check.inset_px, check.tolerance_px]
        .into_iter()
        .flatten()
    {
        if !value.is_finite() || value.abs() > 10_000.0 {
            return Err(invalid(
                "Check offsets, insets and tolerances must be finite px values.",
            ));
        }
    }
    if let Some(label) = &check.label {
        bounded_text("checks[].label", label)?;
    }
    Ok(())
}

/// The id a plan is known by: the SHA-256 of its normalized form, so the same
/// plan always has the same id and any edit gives a new one.
fn plan_id(plan: &PlanInput) -> Result<String, DevupError> {
    let bytes = serde_json::to_vec(plan)
        .map_err(|error| invalid(format!("Cannot serialize the plan: {error}")))?;
    Ok(sha256(&bytes))
}

pub(crate) fn plan(
    input: PlanInput,
    policy: &OutputPolicy,
    output_path: Option<&str>,
) -> Result<Value, DevupError> {
    validate_plan(&input)?;
    let id = plan_id(&input)?;
    let checks = input.screens.iter().map(|s| s.checks.len()).sum::<usize>();
    let references = input
        .screens
        .iter()
        .filter_map(|screen| {
            let figma = screen.figma.as_ref()?;
            let (comparable, layout_only): (Vec<_>, Vec<_>) = input
                .viewports
                .iter()
                .map(|viewport| viewport.width)
                .partition(|width| figma.drawn_widths.contains(width));
            Some(json!({"screen": screen.id, "nodeId": figma.node_id,
                "drawnWidths": figma.drawn_widths, "visualOriginalAt": comparable,
                "layoutChecksOnlyAt": layout_only}))
        })
        .collect::<Vec<_>>();
    let document = json!({
        "schemaVersion": SCHEMA_VERSION,
        "kind": PLAN_KIND,
        "planId": id,
        "createdBy": super::delivery::server_identity(),
        "plan": input,
        "expected": {
            "runs": input.viewports.len(),
            "screensPerRun": input.screens.len(),
            "checkItems": checks * input.viewports.len(),
        },
        "figmaReferences": references,
        "executor": {
            "package": "harness/runtime-audit",
            "install": "npm ci && npx playwright install chromium",
            "run": "node run.mjs --plan <plan.json> --out <report directory>",
            "isolation": "One fresh browser context per viewport, with no profile, cookies or storage from the user's browser. Requests to origins outside allowedOrigins are blocked and recorded.",
        },
    });
    let mut written = None;
    if let Some(path) = output_path {
        let target = policy.resolve(path)?;
        let display = target.display_path().display().to_string();
        let mut transaction = OutputTransaction::new();
        let bytes = serde_json::to_vec_pretty(&document)
            .map_err(|error| invalid(format!("Cannot serialize the plan: {error}")))?;
        transaction.stage("plan", target, &bytes)?;
        transaction.commit()?;
        written = Some(display);
    }
    let mut result = document;
    result["writtenTo"] = json!(written);
    result["nextAction"] = json!({
        "where": "the consumer project",
        "how": "Run the executor against this plan with the app served at baseUrl, then call devup_runtime_audit with action ingest, the plan path and the report.json it writes. Nothing here has run yet.",
    });
    Ok(result)
}

fn read_json(
    policy: &OutputPolicy,
    path: &str,
    limit: usize,
) -> Result<(Vec<u8>, Value, std::path::PathBuf), DevupError> {
    let target = policy.resolve(path)?;
    let bytes = target.read_bounded(limit)?;
    let value = serde_json::from_slice(&bytes)
        .map_err(|error| invalid(format!("{path} is not valid JSON: {error}")))?;
    Ok((bytes, value, target.display_path().to_path_buf()))
}

fn rejected(reason: &str, details: Value) -> Value {
    json!({
        "verdict": "rejected",
        "reason": reason,
        "details": details,
        "note": "Nothing in this report was counted. A report that cannot be tied to the plan it claims to run is not evidence of that plan.",
    })
}

pub(crate) fn ingest(
    policy: &OutputPolicy,
    plan_path: &str,
    report_path: &str,
) -> Result<Value, DevupError> {
    let (plan_bytes, plan_document, _) = read_json(policy, plan_path, MAX_PLAN_BYTES)?;
    if plan_document["kind"] != PLAN_KIND || plan_document["schemaVersion"] != SCHEMA_VERSION {
        return Ok(rejected(
            "plan-kind",
            json!({"expected": PLAN_KIND, "schemaVersion": SCHEMA_VERSION}),
        ));
    }
    let Ok(plan) = serde_json::from_value::<PlanInput>(plan_document["plan"].clone()) else {
        return Ok(rejected("plan-schema", json!({"path": plan_path})));
    };
    if let Err(error) = validate_plan(&plan) {
        return Ok(rejected("plan-invalid", json!({"message": error.message})));
    }
    let id = plan_id(&plan)?;
    if plan_document["planId"] != id.as_str() {
        return Ok(rejected(
            "plan-edited",
            json!({"declared": plan_document["planId"], "recomputed": id}),
        ));
    }
    let (_, report, report_file) = read_json(policy, report_path, MAX_REPORT_BYTES)?;
    if report["kind"] != REPORT_KIND || report["schemaVersion"] != SCHEMA_VERSION {
        return Ok(rejected(
            "report-kind",
            json!({"expected": REPORT_KIND, "schemaVersion": SCHEMA_VERSION}),
        ));
    }
    if report["planId"] != id.as_str() {
        return Ok(rejected(
            "report-for-another-plan",
            json!({"planId": id, "reportPlanId": report["planId"]}),
        ));
    }
    let plan_file_hash = sha256(&plan_bytes);
    let plan_file_matches = report["planSha256"]
        .as_str()
        .map(|declared| declared == plan_file_hash);
    if plan_file_matches == Some(false) {
        return Ok(rejected(
            "plan-file-differs",
            json!({"planSha256": plan_file_hash, "reportPlanSha256": report["planSha256"]}),
        ));
    }
    let report_dir = report_file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();

    let mut counts = BTreeMap::<&str, usize>::new();
    let mut failures = Vec::new();
    let mut runs = Vec::new();
    let mut screenshot_budget = MAX_SCREENSHOTS;
    let mut integrity_problems = Vec::new();
    let mut environment_invalid = false;
    let mut visual_comparisons = Vec::new();
    for viewport in &plan.viewports {
        let run = report["runs"].as_array().into_iter().flatten().find(|run| {
            run["viewport"]["width"] == viewport.width
                && run["viewport"]["height"] == viewport.height
                && run["viewport"]["deviceScaleFactor"]
                    .as_f64()
                    .is_some_and(|scale| (scale - viewport.device_scale_factor).abs() < 1e-9)
        });
        let environment_errors = run
            .map(|run| {
                run["environmentErrors"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        if run.is_none() || !environment_errors.is_empty() {
            environment_invalid = true;
        }
        let mut screenshots_verified = 0;
        let mut screenshot_issues = Vec::new();
        let mut drawn_width_shots = Vec::new();
        for screen in &plan.screens {
            let reported = run.and_then(|run| {
                run["screens"]
                    .as_array()?
                    .iter()
                    .find(|candidate| candidate["id"] == screen.id.as_str())
            });
            for (index, check) in screen.checks.iter().enumerate() {
                let item = reported.and_then(|screen| {
                    screen["checks"]
                        .as_array()?
                        .iter()
                        .find(|item| item["index"] == index)
                });
                let status = match item.and_then(|item| item["status"].as_str()) {
                    Some(status @ ("pass" | "fail" | "inconclusive" | "skipped")) => status,
                    Some(_) => "inconclusive",
                    None => "not-run",
                };
                *counts.entry(status).or_default() += 1;
                if status == "fail" && failures.len() < MAX_LISTED {
                    failures.push(json!({
                        "viewport": {"width": viewport.width, "height": viewport.height},
                        "screen": screen.id,
                        "check": index,
                        "kind": check.kind,
                        "label": check.label,
                        "reason": item.map(|item| &item["reason"]),
                        "measured": item.map(|item| &item["measured"]),
                    }));
                }
            }
            let Some(shot) = reported
                .map(|screen| &screen["screenshot"])
                .filter(|shot| shot.is_object())
            else {
                continue;
            };
            if screenshot_budget == 0 {
                screenshot_issues.push(
                    json!({"screen": screen.id, "issue": "not-verified: screenshot limit reached"}),
                );
                continue;
            }
            screenshot_budget -= 1;
            match verify_screenshot(policy, &report_dir, shot, viewport) {
                Ok(verified) => {
                    screenshots_verified += 1;
                    if let Some(figma) = screen
                        .figma
                        .as_ref()
                        .filter(|figma| figma.drawn_widths.contains(&viewport.width))
                    {
                        drawn_width_shots.push((screen, figma.node_id.as_str(), verified));
                    }
                }
                Err(Integrity::Tampered(issue)) => {
                    integrity_problems.push(json!({"screen": screen.id, "issue": issue}));
                }
                Err(Integrity::Unverified(issue)) => {
                    screenshot_issues.push(json!({"screen": screen.id, "issue": issue}));
                }
            }
        }
        if !screenshot_issues.is_empty() {
            environment_invalid = true;
        }
        let run_valid =
            run.is_some() && environment_errors.is_empty() && screenshot_issues.is_empty();
        for (screen, node_id, shot) in &drawn_width_shots {
            visual_comparisons.push(visual_comparison(
                screen, node_id, viewport, shot, &report, run_valid,
            ));
        }
        runs.push(json!({
            "viewport": viewport,
            "reported": run.is_some(),
            "environmentErrors": environment_errors,
            "environmentValid": run.is_some() && environment_errors.is_empty(),
            "screenshots": {"verified": screenshots_verified, "issues": screenshot_issues},
        }));
    }
    if !integrity_problems.is_empty() {
        return Ok(rejected(
            "artifact-changed",
            json!({"artifacts": integrity_problems}),
        ));
    }
    let planned = plan.viewports.len() * plan.screens.iter().map(|s| s.checks.len()).sum::<usize>();
    let count = |status: &str| counts.get(status).copied().unwrap_or(0);
    let verdict = if count("fail") > 0 && !environment_invalid {
        "fail"
    } else if count("pass") == planned && !environment_invalid && report["status"] == "completed" {
        "pass"
    } else {
        "inconclusive"
    };
    let declared = &report["summary"];
    let declared_matches = declared.is_object().then(|| {
        ["pass", "fail", "inconclusive", "skipped"]
            .iter()
            .all(|status| declared[*status].as_u64().unwrap_or(0) == count(status) as u64)
    });
    let figma = plan
        .screens
        .iter()
        .filter_map(|screen| {
            let figma = screen.figma.as_ref()?;
            let (visual, layout_only): (Vec<_>, Vec<_>) = plan
                .viewports
                .iter()
                .map(|viewport| viewport.width)
                .partition(|width| figma.drawn_widths.contains(width));
            Some(json!({"screen": screen.id, "nodeId": figma.node_id,
                "visualOriginalAt": visual, "layoutChecksOnlyAt": layout_only,
                "note": "Compare screenshots with devup_visual_compare only at the drawn widths - visualComparisons holds the call for each one; elsewhere there is no visual original and only the layout checks above apply."}))
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "verdict": verdict,
        "planId": id,
        "integrity": {"planIdRecomputed": true, "planFileHashMatches": plan_file_matches},
        "coverage": {"plannedCheckItems": planned, "notRun": count("not-run"),
            "reportStatus": report["status"]},
        "summary": {"pass": count("pass"), "fail": count("fail"),
            "inconclusive": count("inconclusive"), "skipped": count("skipped"),
            "notRun": count("not-run")},
        "declaredSummaryMatches": declared_matches,
        "failures": failures,
        "runs": runs,
        "figmaReferences": figma,
        "visualComparisons": visual_comparisons,
        "executor": report["executor"],
        "environment": report["environment"],
        "evidence": {
            "independentlyVerified": [
                "the plan file's content id and SHA-256",
                "the report's binding to that plan",
                "every count above, recomputed from the reported items",
                "each screenshot's SHA-256, byte length, PNG decoding and width",
            ],
            "executorDeclared": [
                "element geometry and each check's outcome",
                "page, console and network errors",
                "timing, browser and environment versions",
            ],
            "notJudged": "Whether the screens look right to a person. Layout checks measure the relations the plan names and nothing else.",
        },
    }))
}

enum Integrity {
    Tampered(String),
    Unverified(String),
}

struct VerifiedShot {
    path: String,
    sha256: String,
    width: u32,
    height: u32,
}

fn verify_screenshot(
    policy: &OutputPolicy,
    report_dir: &Path,
    shot: &Value,
    viewport: &ViewportInput,
) -> Result<VerifiedShot, Integrity> {
    let path = shot["path"]
        .as_str()
        .ok_or_else(|| Integrity::Unverified("no path".into()))?;
    let relative = Path::new(path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(Integrity::Unverified(format!(
            "{path} must be relative to the report directory, without .. or a root"
        )));
    }
    let joined = report_dir.join(relative);
    let target = policy
        .resolve(&joined.to_string_lossy())
        .map_err(|error| Integrity::Unverified(format!("{path}: {}", error.message)))?;
    let bytes = target
        .read_bounded(MAX_SCREENSHOT_BYTES)
        .map_err(|error| Integrity::Unverified(format!("{path}: {}", error.message)))?;
    let digest = sha256(&bytes);
    if shot["sha256"].as_str() != Some(digest.as_str())
        || shot["bytes"].as_u64() != Some(bytes.len() as u64)
    {
        return Err(Integrity::Tampered(format!(
            "{path}: its SHA-256 or length differs from the report"
        )));
    }
    let [width, height] = devup_mcp_visual::png_dimensions(&bytes).map_err(|_| {
        Integrity::Unverified(format!(
            "{path}: not a decodable PNG within 8192px per side"
        ))
    })?;
    let expected = (f64::from(viewport.width) * viewport.device_scale_factor).round() as u32;
    let minimum = (f64::from(viewport.height) * viewport.device_scale_factor).round() as u32;
    if width != expected || height < minimum {
        return Err(Integrity::Unverified(format!(
            "{path}: {width}x{height} is not {expected} wide and at least {minimum} tall; the capture does not show this viewport"
        )));
    }
    Ok(VerifiedShot {
        path: target.display_path().display().to_string(),
        sha256: digest,
        width,
        height,
    })
}

/// The `devup_visual_compare` call for one screenshot taken at a width its
/// Figma frame was drawn at, with every field the audit itself knows filled
/// in, the ones only the consumer can state named, and what would still make
/// it inconclusive said up front.
fn visual_comparison(
    screen: &ScreenInput,
    node_id: &str,
    viewport: &ViewportInput,
    shot: &VerifiedShot,
    report: &Value,
    environment_valid: bool,
) -> Value {
    let mut environment = json!({
        "schemaVersion": 1,
        "actualPng": shot.path,
        "viewport": {"width": viewport.width, "height": viewport.height,
            "deviceScaleFactor": viewport.device_scale_factor},
        "environmentStatus": if environment_valid { "valid" } else { "environment-invalid" },
    });
    let mut must_add = vec![
        "reference.artifactId",
        "environment.tsxResource",
        "environment.themePath",
        "environment.assetDirectory",
        "environment.devupUiVersion",
        "environment.fontManifest",
    ];
    let browser = &report["executor"]["browser"];
    match (browser["name"].as_str(), browser["version"].as_str()) {
        (Some(name), Some(version)) => {
            environment["renderer"] =
                json!({"name": format!("playwright-{name}"), "version": version});
        }
        _ => must_add.push("environment.renderer"),
    }
    for (field, value) in [
        ("os", &report["environment"]["os"]),
        ("locale", &report["environment"]["locale"]),
        ("timezone", &report["environment"]["timezone"]),
    ] {
        match value.as_str() {
            Some(text) => environment[field] = json!(text),
            None => must_add.push(match field {
                "os" => "environment.os",
                "locale" => "environment.locale",
                _ => "environment.timezone",
            }),
        }
    }
    let mut blockers = Vec::new();
    if (viewport.device_scale_factor - 1.0).abs() > f64::EPSILON {
        blockers.push(format!(
            "Captured at deviceScaleFactor {}; the Figma reference is at scale 1, so plan this width at 1.",
            viewport.device_scale_factor
        ));
    }
    if shot.height != viewport.height {
        blockers.push(format!(
            "The page is {}px tall in a {}px viewport, so the full-page capture is {}x{}. devup_visual_compare needs the viewport to be the reference's size: plan this width at the frame's drawn height.",
            shot.height, viewport.height, shot.width, shot.height
        ));
    }
    if !environment_valid {
        blockers.push(
            "This run reported environment errors or an unverified screenshot; the comparison stays inconclusive."
                .to_owned(),
        );
    }
    json!({
        "screen": screen.id,
        "nodeId": node_id,
        "viewport": viewport,
        "screenshot": {"path": shot.path, "sha256": shot.sha256, "width": shot.width, "height": shot.height},
        "call": {
            "tool": "devup_visual_compare",
            "arguments": {
                "actual": shot.path,
                "reference": {"artifactId": format!("<artifactId of devup_figma_export for node {node_id} with outputs [\"referencePng\"]>")},
                "environment": environment,
            },
        },
        "consumerMustAdd": must_add,
        "blockers": blockers,
        "note": "Filled from the plan, the verified screenshot and the executor's report. Export the frame with outputs [\"referencePng\"] first and put its artifactId in reference; devup_visual_compare pins that reference itself. The fields in consumerMustAdd are the consumer's to state.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_input() -> PlanInput {
        serde_json::from_value(json!({
            "baseUrl": "http://127.0.0.1:4173",
            "allowedOrigins": ["http://127.0.0.1:4173"],
            "viewports": [{"width": 360, "height": 740}, {"width": 1200, "height": 800}],
            "screens": [
                {"id": "today", "routePath": "/today",
                 "figma": {"nodeId": "1:138", "drawnWidths": [360]},
                 "checks": [
                    {"kind": "no-horizontal-overflow"},
                    {"kind": "contained", "subject": {"testId": "badge-text"}, "container": {"testId": "badge"}}
                 ]},
                {"id": "next", "steps": [{"action": "click", "target": {"role": "button", "name": "Next"}}],
                 "expect": {"urlPath": "/next"},
                 "checks": [{"kind": "aligned", "edge": "right", "offsetPx": 36,
                    "subject": {"testId": "qr"}, "container": {"testId": "card"}}]}
            ]
        }))
        .unwrap()
    }

    fn root() -> (std::path::PathBuf, OutputPolicy) {
        let dir = std::env::temp_dir().join(format!(
            "devup-runtime-audit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let policy = OutputPolicy::from_roots(vec![dir.clone()]).unwrap();
        (dir, policy)
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(width, height, image::Rgba([255, 255, 255, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    fn report_for(
        dir: &Path,
        plan_file: &[u8],
        id: &str,
        check_status: &str,
        height_1200: u32,
    ) -> Value {
        let mut runs = Vec::new();
        for (width, height) in [(360_u32, 740_u32), (1200, height_1200)] {
            let mut screens = Vec::new();
            for (screen, checks) in [("today", 2), ("next", 1)] {
                let name = format!("{width}-{screen}.png");
                let bytes = png(width, height.max(740));
                std::fs::write(dir.join(&name), &bytes).unwrap();
                screens.push(json!({"id": screen, "status": "completed",
                    "checks": (0..checks).map(|index| json!({"index": index, "status": check_status, "reason": "measured"})).collect::<Vec<_>>(),
                    "screenshot": {"path": name, "sha256": sha256(&bytes), "bytes": bytes.len()}}));
            }
            runs.push(json!({"viewport": {"width": width, "height": if width == 360 { 740 } else { 800 }, "deviceScaleFactor": 1},
                "environmentErrors": [], "screens": screens}));
        }
        json!({"schemaVersion": 1, "kind": REPORT_KIND, "planId": id, "planSha256": sha256(plan_file),
            "status": "completed", "runs": runs, "summary": {"pass": 6, "fail": 0, "inconclusive": 0, "skipped": 0}})
    }

    fn write_plan(dir: &Path, policy: &OutputPolicy) -> (String, Vec<u8>) {
        let document = plan(
            plan_input(),
            policy,
            Some(&dir.join("plan.json").to_string_lossy()),
        )
        .unwrap();
        let bytes = std::fs::read(dir.join("plan.json")).unwrap();
        (document["planId"].as_str().unwrap().to_owned(), bytes)
    }

    #[test]
    fn a_plan_is_identified_by_its_content_and_refuses_what_is_not_declarative() {
        let first = plan_id(&plan_input()).unwrap();
        assert_eq!(first, plan_id(&plan_input()).unwrap());
        let mut edited = plan_input();
        edited.viewports[0].width = 361;
        assert_ne!(first, plan_id(&edited).unwrap());

        let shell = serde_json::from_value::<PlanInput>(json!({
            "baseUrl": "http://127.0.0.1:4173", "allowedOrigins": ["http://127.0.0.1:4173"],
            "viewports": [{"width": 360, "height": 740}],
            "screens": [{"id": "a", "routePath": "/", "command": "rm -rf /"}]
        }));
        assert!(
            shell.is_err(),
            "unknown fields such as a command are refused"
        );

        for (field, value) in [
            ("baseUrl", json!("file:///etc/passwd")),
            ("allowedOrigins", json!(["http://evil.example"])),
            ("viewports", json!([{"width": 99999, "height": 740}])),
        ] {
            let mut raw = serde_json::to_value(plan_input()).unwrap();
            raw[field] = value;
            let parsed: PlanInput = serde_json::from_value(raw).unwrap();
            assert!(validate_plan(&parsed).is_err(), "{field} must be refused");
        }
        let mut css_step = plan_input();
        css_step.screens[1].steps[0].target = LocatorInput {
            css: Some("button".into()),
            ..LocatorInput::default()
        };
        assert!(
            validate_plan(&css_step).is_err(),
            "steps use role/testId/text"
        );

        let mut unverifiable = plan_input();
        unverifiable.viewports = vec![unverifiable.viewports[0].clone(); MAX_VIEWPORTS];
        let screen = unverifiable.screens[0].clone();
        unverifiable.screens = (0..MAX_SCREENS)
            .map(|index| ScreenInput {
                id: format!("screen-{index}"),
                ..screen.clone()
            })
            .collect();
        assert!(
            validate_plan(&unverifiable).is_err(),
            "a plan with more screenshots than ingest verifies could never pass"
        );
    }

    #[test]
    fn a_complete_passing_report_passes_and_counts_are_recomputed() {
        let (dir, policy) = root();
        let (id, plan_file) = write_plan(&dir, &policy);
        let report = report_for(&dir, &plan_file, &id, "pass", 800);
        std::fs::write(
            dir.join("report.json"),
            serde_json::to_vec(&report).unwrap(),
        )
        .unwrap();
        let result = ingest(
            &policy,
            &dir.join("plan.json").to_string_lossy(),
            &dir.join("report.json").to_string_lossy(),
        )
        .unwrap();
        assert_eq!(result["verdict"], "pass", "{result}");
        assert_eq!(result["summary"]["pass"], 6);
        assert_eq!(result["declaredSummaryMatches"], true);
        assert_eq!(
            result["figmaReferences"][0]["visualOriginalAt"],
            json!([360])
        );
        assert_eq!(
            result["figmaReferences"][0]["layoutChecksOnlyAt"],
            json!([1200])
        );
        drop(policy);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Only the screen's drawn width has a visual original, and its verified
    /// screenshot comes back as a ready comparison: what the audit knows is
    /// filled in, what only the consumer knows is named.
    #[test]
    fn a_drawn_width_screenshot_comes_back_as_a_ready_visual_comparison() {
        let (dir, policy) = root();
        let (id, plan_file) = write_plan(&dir, &policy);
        let mut report = report_for(&dir, &plan_file, &id, "pass", 800);
        report["executor"] = json!({"browser": {"name": "chromium", "version": "131.0.1"}});
        report["environment"] = json!({"os": "linux 6.1", "locale": null, "timezone": "UTC"});
        std::fs::write(
            dir.join("report.json"),
            serde_json::to_vec(&report).unwrap(),
        )
        .unwrap();
        let result = ingest(
            &policy,
            &dir.join("plan.json").to_string_lossy(),
            &dir.join("report.json").to_string_lossy(),
        )
        .unwrap();
        let comparisons = result["visualComparisons"].as_array().unwrap();
        assert_eq!(
            comparisons.len(),
            1,
            "only today at 360 was drawn: {result}"
        );
        let comparison = &comparisons[0];
        assert_eq!(comparison["screen"], "today");
        assert_eq!(comparison["nodeId"], "1:138");
        let arguments = &comparison["call"]["arguments"];
        assert_eq!(comparison["call"]["tool"], "devup_visual_compare");
        assert!(
            arguments["actual"]
                .as_str()
                .unwrap()
                .ends_with("360-today.png")
        );
        let environment = &arguments["environment"];
        assert_eq!(environment["actualPng"], arguments["actual"]);
        assert_eq!(environment["environmentStatus"], "valid");
        assert_eq!(
            environment["renderer"],
            json!({"name": "playwright-chromium", "version": "131.0.1"})
        );
        assert_eq!(environment["timezone"], "UTC");
        let must_add = comparison["consumerMustAdd"].as_array().unwrap();
        assert!(must_add.contains(&json!("environment.locale")));
        assert!(must_add.contains(&json!("environment.tsxResource")));
        assert_eq!(comparison["blockers"], json!([]), "{comparison}");
        drop(policy);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failure_fails_missing_items_are_inconclusive_and_edits_are_rejected() {
        let (dir, policy) = root();
        let (id, plan_file) = write_plan(&dir, &policy);
        let plan_path = dir.join("plan.json").to_string_lossy().into_owned();
        let report_path = dir.join("report.json").to_string_lossy().into_owned();

        let failing = report_for(&dir, &plan_file, &id, "fail", 800);
        std::fs::write(&report_path, serde_json::to_vec(&failing).unwrap()).unwrap();
        let result = ingest(&policy, &plan_path, &report_path).unwrap();
        assert_eq!(result["verdict"], "fail");
        assert_eq!(
            result["declaredSummaryMatches"], false,
            "the declared summary claimed passes"
        );

        let mut partial = report_for(&dir, &plan_file, &id, "pass", 800);
        partial["runs"].as_array_mut().unwrap().pop();
        std::fs::write(&report_path, serde_json::to_vec(&partial).unwrap()).unwrap();
        let result = ingest(&policy, &plan_path, &report_path).unwrap();
        assert_eq!(result["verdict"], "inconclusive");
        assert_eq!(result["summary"]["notRun"], 3);

        let mut environment = report_for(&dir, &plan_file, &id, "fail", 800);
        environment["runs"][0]["environmentErrors"] =
            json!([{"kind": "font-missing", "message": "Pretendard"}]);
        std::fs::write(&report_path, serde_json::to_vec(&environment).unwrap()).unwrap();
        assert_eq!(
            ingest(&policy, &plan_path, &report_path).unwrap()["verdict"],
            "inconclusive"
        );

        let cropped = report_for(&dir, &plan_file, &id, "pass", 700);
        std::fs::write(&report_path, serde_json::to_vec(&cropped).unwrap()).unwrap();
        let result = ingest(&policy, &plan_path, &report_path).unwrap();
        assert_eq!(
            result["verdict"], "inconclusive",
            "a shorter capture than the viewport: {result}"
        );

        let tampered = report_for(&dir, &plan_file, &id, "pass", 800);
        std::fs::write(&report_path, serde_json::to_vec(&tampered).unwrap()).unwrap();
        std::fs::write(dir.join("360-today.png"), png(360, 741)).unwrap();
        assert_eq!(
            ingest(&policy, &plan_path, &report_path).unwrap()["reason"],
            "artifact-changed"
        );

        let mut edited: Value = serde_json::from_slice(&plan_file).unwrap();
        edited["plan"]["viewports"][0]["width"] = json!(390);
        std::fs::write(&plan_path, serde_json::to_vec(&edited).unwrap()).unwrap();
        assert_eq!(
            ingest(&policy, &plan_path, &report_path).unwrap()["reason"],
            "plan-edited"
        );
        drop(policy);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn screenshot_paths_cannot_leave_the_report_directory() {
        let (dir, policy) = root();
        let viewport = ViewportInput {
            width: 360,
            height: 740,
            device_scale_factor: 1.0,
            mobile: false,
            touch: false,
        };
        for path in ["../outside.png", "/etc/passwd", "C:\\Windows\\win.ini"] {
            let shot = json!({"path": path, "sha256": "0", "bytes": 1});
            assert!(matches!(
                verify_screenshot(&policy, &dir, &shot, &viewport),
                Err(Integrity::Unverified(_))
            ));
        }
        drop(policy);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
