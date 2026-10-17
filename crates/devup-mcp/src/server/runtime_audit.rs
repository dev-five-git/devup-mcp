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

use super::artifacts::ArtifactStore;
use super::output::{OutputPolicy, OutputTransaction};

pub(crate) const PLAN_KIND: &str = "devup-runtime-audit-plan";
pub(crate) const REPORT_KIND: &str = "devup-runtime-audit-report";
const SCHEMA_VERSION: u64 = 1;
const MAX_VIEWPORTS: usize = 8;
const MAX_SCREENS: usize = 60;
const MAX_STEPS: usize = 20;
// A full audit names a bounds, capture and health check per section and
// asset, so a screen carries far more than a hand-written plan would.
const MAX_CHECKS: usize = 200;
const MAX_TIMEOUT_MS: u64 = 120_000;
const MAX_VIEWPORT_SIDE: u32 = 4096;
const MAX_TEXT: usize = 300;
const MAX_PLAN_BYTES: usize = 1024 * 1024;
const MAX_REPORT_BYTES: usize = 8 * 1024 * 1024;
const MAX_SCREENSHOT_BYTES: usize = 16 * 1024 * 1024;
const MAX_SCREENSHOTS: usize = 400;
const MAX_LISTED: usize = 50;
// Keep complete per-width rows in the response without unbounded duplication.
const MAX_DIMENSION_ROWS: usize = 100;

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
    /// plan: emit the consumer Playwright runner and its pinned npm manifest
    /// into this directory under an allowed root; requires outputPath.
    #[serde(default)]
    pub template_path: Option<String>,
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
    /// Defaults to 1440, 1100, 858 and 390 CSS-pixel widths at height 900.
    #[serde(default = "default_viewports")]
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

fn default_viewports() -> Vec<ViewportInput> {
    [1440, 1100, 858, 390]
        .into_iter()
        .map(|width| ViewportInput {
            width,
            height: 900,
            device_scale_factor: 1.0,
            mobile: false,
            touch: false,
        })
        .collect()
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
    /// dimensions: compare the explicitly mapped element box in CSS pixels
    /// with acquired Figma logical dimensions at each named viewport width.
    /// view-transition: follow the subject link to another route and confirm
    /// a view transition ran (document.startViewTransition, or a
    /// cross-document pagereveal transition). It navigates away, so the
    /// executor runs it after the screen's other checks and screenshot; the
    /// next screen should start from its own routePath. The plan writer
    /// moves it after the screen's other checks and the executor returns to
    /// the screen's URL after it, so order in the plan cannot break a run.
    /// page-health: no error page text ("This page couldn't load",
    /// "Application error", ...), page error, console error or failed
    /// navigation while the screen was open.
    /// bounds: subject's size and position relative to container's top-left,
    /// in CSS px, against `expected` (the Figma geometry) within tolerancePx.
    /// no-clipped-text: no text inside subject (or the page) is cut off by an
    /// overflow-hidden box; ellipsis and line clamp are intended and pass.
    /// interaction: hovering or keyboard-focusing subject (`state`) changes
    /// something visible; without a subject, every visible link and button.
    /// capture: screenshot subject's element box for devup_visual_compare
    /// against `reference`; ingest turns it into a ready call.
    #[schemars(extend("enum" = ["no-horizontal-overflow", "contained", "aligned", "spans", "visible", "dimensions", "view-transition", "page-health", "bounds", "no-clipped-text", "interaction", "capture"]))]
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
    /// Explicit mappings only. Widths omitted here have no design reference;
    /// intermediate widths are never interpolated or inferred from screenshots.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub figma_dimensions: Vec<DimensionReferenceInput>,
    /// bounds: the expected box relative to container, in CSS px.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<BoundsInput>,
    /// interaction: hover or focus (keyboard, so :focus-visible applies).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("enum" = ["hover", "focus"]))]
    pub state: Option<String>,
    /// Only at these viewport widths; empty means every width. A bounds or
    /// capture check holds Figma geometry, which exists only where the frame
    /// was drawn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub widths: Vec<u32>,
    /// The Figma node this check is about, so a failure names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    /// capture: what the element screenshot is compared with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<CaptureReferenceInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct BoundsInput {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A native-node PNG of the same Figma node, captured by devup_figma_export
/// (assetCaptures, renderMode node, format png, scale 1).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct CaptureReferenceInput {
    /// section: a top-level child of the frame. asset: an image or icon.
    #[schemars(extend("enum" = ["section", "asset"]))]
    pub scope: String,
    pub artifact_id: String,
    pub asset_id: String,
    /// Changed-pixel ratio the comparison allows; default 0.03 for a
    /// section and 0.05 for an asset, which absorbs anti-aliased edges but
    /// not a filled shape where an outline was drawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
}

/// Changed-pixel ratios that absorb anti-aliasing between Figma's renderer
/// and Chromium. A small icon has proportionally more edge, so it gets more.
pub(crate) const PAGE_THRESHOLD: f64 = 0.03;
pub(crate) const SECTION_THRESHOLD: f64 = 0.03;
pub(crate) const ASSET_THRESHOLD: f64 = 0.05;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct DimensionReferenceInput {
    pub viewport_width: u32,
    pub artifact_id: String,
    pub node_id: String,
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
    /// The frame drawn at each width when a page has one per width, with
    /// the devup_figma_export artifact holding its referencePng. Ingest puts
    /// that artifactId in the page comparison it returns.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<FrameReferenceInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct FrameReferenceInput {
    pub width: u32,
    pub node_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
    /// Changed-pixel ratio the page comparison allows; default 0.03.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
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
    let mut viewports = std::collections::BTreeSet::new();
    if plan
        .viewports
        .iter()
        .any(|v| !viewports.insert((v.width, v.height, v.device_scale_factor.to_bits())))
    {
        return Err(invalid(
            "Duplicate viewports would make report attribution ambiguous.",
        ));
    }
    if plan.screens.is_empty() || plan.screens.len() > MAX_SCREENS {
        return Err(invalid(format!(
            "screens must hold 1 to {MAX_SCREENS} entries."
        )));
    }
    let dimension_rows = plan.viewports.len()
        * plan
            .screens
            .iter()
            .map(|screen| {
                screen
                    .checks
                    .iter()
                    .filter(|check| check.kind == "dimensions")
                    .count()
            })
            .sum::<usize>();
    if dimension_rows > MAX_DIMENSION_ROWS {
        return Err(invalid(format!(
            "A plan may contain at most {MAX_DIMENSION_ROWS} dimension rows (dimension checks x viewports); split the plan to keep every width row explicit."
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
            if check
                .figma_dimensions
                .iter()
                .any(|r| !plan.viewports.iter().any(|v| v.width == r.viewport_width))
            {
                return Err(invalid(
                    "figmaDimensions.viewportWidth must name a planned viewport width.",
                ));
            }
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
        "no-horizontal-overflow" | "page-health" => needs(false, false)?,
        "visible" | "view-transition" => needs(true, false)?,
        "no-clipped-text" => {
            if check.container.is_some() {
                return Err(invalid("A no-clipped-text check takes no container."));
            }
        }
        "bounds" => {
            needs(true, true)?;
            let Some(expected) = &check.expected else {
                return Err(invalid(
                    "A bounds check needs expected {x, y, width, height}.",
                ));
            };
            if [expected.x, expected.y, expected.width, expected.height]
                .iter()
                .any(|value| !value.is_finite() || value.abs() > 100_000.0)
                || expected.width < 0.0
                || expected.height < 0.0
            {
                return Err(invalid(
                    "bounds expected values must be finite px, width and height nonnegative.",
                ));
            }
        }
        "interaction" => {
            // Without a subject it sweeps every visible link and button.
            if check.container.is_some() {
                return Err(invalid("An interaction check takes no container."));
            }
            if !matches!(check.state.as_deref(), Some("hover" | "focus")) {
                return Err(invalid("An interaction check needs state hover or focus."));
            }
        }
        "capture" => {
            needs(true, false)?;
            let Some(reference) = &check.reference else {
                return Err(invalid(
                    "A capture check needs reference {scope, artifactId, assetId}.",
                ));
            };
            if !matches!(reference.scope.as_str(), "section" | "asset") {
                return Err(invalid("capture reference.scope must be section or asset."));
            }
            bounded_text("reference.artifactId", &reference.artifact_id)?;
            bounded_text("reference.assetId", &reference.asset_id)?;
            if reference
                .threshold
                .is_some_and(|value| !(0.0..=1.0).contains(&value))
            {
                return Err(invalid("reference.threshold must be between 0 and 1."));
            }
        }
        "dimensions" => {
            needs(true, false)?;
            if check.tolerance_px.is_none()
                || check.figma_dimensions.is_empty()
                || !matches!(check.measure.as_deref(), None | Some("box"))
            {
                return Err(invalid(
                    "A dimensions check needs tolerancePx, figmaDimensions and measure box.",
                ));
            }
        }
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
    for (field, present, kind) in [
        ("expected", check.expected.is_some(), "bounds"),
        ("state", check.state.is_some(), "interaction"),
        ("reference", check.reference.is_some(), "capture"),
    ] {
        if present && check.kind != kind {
            return Err(invalid(format!("{field} is only valid for {kind} checks.")));
        }
    }
    if let Some(node_id) = &check.node_id {
        bounded_text("checks[].nodeId", node_id)?;
    }
    if check.kind != "dimensions" && !check.figma_dimensions.is_empty() {
        return Err(invalid(
            "figmaDimensions is only valid for dimensions checks.",
        ));
    }
    let mut widths = std::collections::BTreeSet::new();
    for reference in &check.figma_dimensions {
        bounded_text("figmaDimensions.artifactId", &reference.artifact_id)?;
        bounded_text("figmaDimensions.nodeId", &reference.node_id)?;
        if !widths.insert(reference.viewport_width) {
            return Err(invalid(
                "A dimensions check needs at most one explicit reference per width.",
            ));
        }
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
    if check.tolerance_px.is_some_and(|value| value < 0.0) {
        return Err(invalid("tolerancePx must be nonnegative."));
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

#[cfg(test)]
fn plan(
    input: PlanInput,
    policy: &OutputPolicy,
    output_path: Option<&str>,
) -> Result<Value, DevupError> {
    write_plan(input, policy, output_path, None, Vec::new())
}

/// Resolve design dimensions from retained acquisitions, never caller numbers.
/// Missing/expired artifacts remain explicit so they cannot become a passing check.
async fn dimension_references(input: &PlanInput, artifacts: &ArtifactStore) -> Vec<Value> {
    let mut references = Vec::new();
    for screen in &input.screens {
        for (index, check) in screen.checks.iter().enumerate() {
            for reference in &check.figma_dimensions {
                let mut value = json!({"screen": screen.id, "check": index,
                    "viewportWidth": reference.viewport_width, "artifactId": reference.artifact_id,
                    "nodeId": reference.node_id, "status": "unverified",
                    "reason": "The retained acquisition is missing or expired; acquire the node again.",
                    "mapping": "caller-declared", "units": "figma-logical-px"});
                if let Some(artifact) = artifacts.get(&reference.artifact_id).await {
                    value["artifactContentHash"] = json!(artifact.content_hash);
                    value["staleness"] = json!("retained snapshot; not compared with live Figma");
                    value["reason"] =
                        json!("The acquired node has no verified logical width and height.");
                    if let Some(node) = artifact.payload.snapshot.nodes.get(&reference.node_id) {
                        let view = node.typed_view();
                        let width = (!node.field_errors.contains_key("width"))
                            .then(|| view.number("width"))
                            .flatten()
                            .or_else(|| {
                                (!node.field_errors.contains_key("size"))
                                    .then(|| view.value("size")?.get("x")?.as_f64())
                                    .flatten()
                            });
                        let height = (!node.field_errors.contains_key("height"))
                            .then(|| view.number("height"))
                            .flatten()
                            .or_else(|| {
                                (!node.field_errors.contains_key("size"))
                                    .then(|| view.value("size")?.get("y")?.as_f64())
                                    .flatten()
                            });
                        if let (Some(width), Some(height)) = (width, height)
                            && width.is_finite()
                            && height.is_finite()
                            && width >= 0.0
                            && height >= 0.0
                        {
                            value["status"] = json!("acquired");
                            value["width"] = json!(width);
                            value["height"] = json!(height);
                            value["reason"] = Value::Null;
                            value["source"] = json!(
                                "node logical width/height or size; no screenshot, zoom, DPR or bounding-box inference"
                            );
                        }
                    }
                }
                references.push(value);
            }
        }
    }
    references
}

pub(crate) async fn create_plan(
    mut input: PlanInput,
    policy: &OutputPolicy,
    output_path: Option<&str>,
    template_path: Option<&str>,
    artifacts: &ArtifactStore,
) -> Result<Value, DevupError> {
    order_checks(&mut input);
    validate_plan(&input)?;
    let references = dimension_references(&input, artifacts).await;
    write_plan(input, policy, output_path, template_path, references)
}

/// One screen at one width by what was checked: health (renders, no
/// overflow, no clipped text), geometry (size and position), interaction
/// (states and view transitions), and the three visual levels, which stay
/// "to compare" until devup_visual_compare results come back through
/// devup_screen_plan status.
fn columns(
    screen: &ScreenInput,
    viewport: &ViewportInput,
    judged: &[(&str, &CheckInput)],
    row: &Value,
) -> Value {
    let fold = |kinds: &[&str]| {
        let statuses = judged
            .iter()
            .filter(|(_, check)| kinds.contains(&check.kind.as_str()))
            .map(|(status, _)| *status)
            .collect::<Vec<_>>();
        if statuses.is_empty() {
            "-"
        } else if statuses.contains(&"fail") {
            "fail"
        } else if statuses.iter().all(|status| *status == "pass") {
            "pass"
        } else if statuses.contains(&"inconclusive") {
            "inconclusive"
        } else {
            "not-run"
        }
    };
    let unreached = row["reasons"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|reason| reason.starts_with("page error") || reason.starts_with("screen not reached"));
    let health = if unreached {
        "fail"
    } else {
        fold(&["page-health", "no-horizontal-overflow", "no-clipped-text"])
    };
    let captures = |scope: &str| {
        let count = judged
            .iter()
            .filter(|(status, check)| {
                *status == "pass"
                    && check
                        .reference
                        .as_ref()
                        .is_some_and(|reference| reference.scope == scope)
            })
            .count();
        let missing = judged
            .iter()
            .filter(|(status, check)| {
                *status != "pass"
                    && check
                        .reference
                        .as_ref()
                        .is_some_and(|reference| reference.scope == scope)
            })
            .count();
        match (count, missing) {
            (0, 0) => json!("-"),
            (_, 0) => json!(format!("{count} to compare")),
            _ => json!(format!("{count} to compare, {missing} not captured")),
        }
    };
    let drawn = screen
        .figma
        .as_ref()
        .is_some_and(|figma| figma.drawn_widths.contains(&viewport.width));
    json!({
        "health": health,
        "geometry": fold(&["bounds", "dimensions", "contained", "aligned", "spans", "visible"]),
        "interaction": fold(&["interaction", "view-transition"]),
        "page": if drawn { json!(if row["screenshot"].is_string() { "1 to compare" } else { "not captured" }) } else { json!("not drawn") },
        "section": captures("section"),
        "asset": captures("asset"),
    })
}

/// A check limited to some widths does not exist at the others.
fn applies(check: &CheckInput, width: u32) -> bool {
    check.widths.is_empty() || check.widths.contains(&width)
}

fn planned_items(plan: &PlanInput) -> usize {
    plan.viewports
        .iter()
        .map(|viewport| {
            plan.screens
                .iter()
                .flat_map(|screen| &screen.checks)
                .filter(|check| applies(check, viewport.width))
                .count()
        })
        .sum()
}

/// A view-transition check clicks away from its screen, so every one moves
/// behind the screen's other checks, keeping their order otherwise. The
/// executor also returns to the screen's URL after it; together a plan
/// cannot measure the wrong page whatever order it was written in.
pub(crate) fn order_checks(input: &mut PlanInput) {
    for screen in &mut input.screens {
        screen
            .checks
            .sort_by_key(|check| check.kind == "view-transition");
    }
}

fn write_plan(
    input: PlanInput,
    policy: &OutputPolicy,
    output_path: Option<&str>,
    template_path: Option<&str>,
    dimension_references: Vec<Value>,
) -> Result<Value, DevupError> {
    validate_plan(&input)?;
    if template_path.is_some() && output_path.is_none() {
        return Err(invalid(
            "templatePath requires outputPath for the bound plan JSON.",
        ));
    }
    let id = plan_id(&input)?;
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
            "checkItems": planned_items(&input),
            "dimensionRows": input.viewports.len() * input.screens.iter().map(|s| s.checks.iter().filter(|c| c.kind == "dimensions").count()).sum::<usize>(),
        },
        "limits": {"dimensionRows": MAX_DIMENSION_ROWS, "dimensionRowsOmitted": 0},
        "figmaReferences": references,
        "dimensionReferences": dimension_references,
        "executor": {
            "package": "harness/runtime-audit",
            "install": "npm ci && npx playwright install chromium",
            "run": "node run.mjs --plan <plan.json> --out <report directory>",
            "isolation": "One fresh browser context per viewport, with no profile, cookies or storage from the user's browser. Requests to origins outside allowedOrigins are blocked and recorded.",
        },
    });
    let mut written = None;
    let mut template_files = Vec::new();
    let mut transaction = OutputTransaction::new();
    if let Some(directory) = template_path {
        transaction.refuse_existing();
        for (name, bytes) in [
            (
                "run.mjs",
                include_bytes!("../../../../harness/runtime-audit/run.mjs").as_slice(),
            ),
            (
                "checks.mjs",
                include_bytes!("../../../../harness/runtime-audit/checks.mjs").as_slice(),
            ),
            (
                "package.json",
                include_bytes!("../../../../harness/runtime-audit/package.json").as_slice(),
            ),
            (
                "package-lock.json",
                include_bytes!("../../../../harness/runtime-audit/package-lock.json").as_slice(),
            ),
        ] {
            let requested = Path::new(directory).join(name);
            let target = policy.resolve(&requested.to_string_lossy())?;
            template_files.push(json!({"path": target.display_path().display().to_string(), "sha256": sha256(bytes)}));
            transaction.stage(name, target, bytes)?;
        }
    }
    if let Some(path) = output_path {
        let target = policy.resolve(path)?;
        let display = target.display_path().display().to_string();
        let bytes = serde_json::to_vec_pretty(&document)
            .map_err(|error| invalid(format!("Cannot serialize the plan: {error}")))?;
        if bytes.len() > MAX_PLAN_BYTES {
            return Err(invalid(
                "The bound plan exceeds the ingest size limit; split the plan.",
            ));
        }
        transaction.stage("plan", target, &bytes)?;
        written = Some(display);
    }
    transaction.commit()?;
    let mut result = document;
    result["writtenTo"] = json!(written);
    result["template"] = json!({"files": template_files, "executed": false,
        "workingDirectory": template_path,
        "commands": [ ["npm", "ci"], ["npx", "playwright", "install", "chromium"],
            ["node", "run.mjs", "--plan", written.as_deref().unwrap_or("<saved plan.json>"), "--out", "report"] ],
        "note": "Execute these argument arrays in the generated directory in your consumer project with its app running. The server executes nothing. The plan and report must remain under allowed roots for ingest."});
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

#[cfg(test)]
fn ingest(policy: &OutputPolicy, plan_path: &str, report_path: &str) -> Result<Value, DevupError> {
    ingest_references(policy, plan_path, report_path, None)
}

pub(crate) async fn ingest_with_artifacts(
    policy: &OutputPolicy,
    plan_path: &str,
    report_path: &str,
    artifacts: &ArtifactStore,
) -> Result<Value, DevupError> {
    let read_policy = policy.clone();
    let read_path = plan_path.to_owned();
    let (_, document, _) =
        tokio::task::spawn_blocking(move || read_json(&read_policy, &read_path, MAX_PLAN_BYTES))
            .await
            .map_err(|_| invalid("The plan could not be read to the end."))??;
    let references = match serde_json::from_value::<PlanInput>(document["plan"].clone()) {
        Ok(plan) if validate_plan(&plan).is_ok() => dimension_references(&plan, artifacts).await,
        Ok(_) => Vec::new(),
        Err(_) => Vec::new(),
    };
    let policy = policy.clone();
    let plan_path = plan_path.to_owned();
    let report_path = report_path.to_owned();
    tokio::task::spawn_blocking(move || {
        ingest_references(&policy, &plan_path, &report_path, Some(&references))
    })
    .await
    .map_err(|_| invalid("The report could not be read to the end."))?
}

fn ingest_references(
    policy: &OutputPolicy,
    plan_path: &str,
    report_path: &str,
    acquired: Option<&[Value]>,
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
    let mut dimension_rows = Vec::new();
    let mut screen_rows = Vec::new();
    let mut findings = Vec::new();
    let mut missing_nodes = Vec::new();
    for viewport in &plan.viewports {
        let matching_runs = report["runs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|run| {
                run["viewport"]["width"] == viewport.width
                    && run["viewport"]["height"] == viewport.height
                    && run["viewport"]["deviceScaleFactor"].as_f64()
                        == Some(viewport.device_scale_factor)
            })
            .count();
        if matching_runs > 1 {
            return Ok(rejected(
                "ambiguous-report-items",
                json!({"viewport": viewport}),
            ));
        }
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
        let mut captures = Vec::new();
        for screen in &plan.screens {
            if run.is_some_and(|run| {
                run["screens"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|s| s["id"] == screen.id)
                    .count()
                    > 1
            }) {
                return Ok(rejected(
                    "ambiguous-report-items",
                    json!({"screen": screen.id}),
                ));
            }
            let reported = run.and_then(|run| {
                run["screens"]
                    .as_array()?
                    .iter()
                    .find(|candidate| candidate["id"] == screen.id.as_str())
            });
            let mut outcomes = Vec::new();
            let mut judged = Vec::new();
            for (index, check) in screen.checks.iter().enumerate() {
                if !applies(check, viewport.width) {
                    continue;
                }
                if reported.is_some_and(|s| {
                    s["checks"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|item| item["index"] == index)
                        .count()
                        > 1
                }) {
                    return Ok(rejected(
                        "ambiguous-report-items",
                        json!({"screen": screen.id, "check": index}),
                    ));
                }
                let item = reported.and_then(|screen| {
                    screen["checks"]
                        .as_array()?
                        .iter()
                        .find(|item| item["index"] == index)
                });
                let dimension = (check.kind == "dimensions").then(|| {
                    dimension_row(
                        screen,
                        index,
                        check,
                        viewport,
                        item,
                        &plan_document,
                        acquired,
                        plan_file_matches == Some(true),
                        run.is_some()
                            && environment_errors.is_empty()
                            && reported.is_some_and(|s| s["status"] == "completed"),
                    )
                });
                let status = match dimension
                    .as_ref()
                    .map(|row| &row["status"])
                    .or_else(|| item.map(|item| &item["status"]))
                    .and_then(Value::as_str)
                {
                    Some("pass") => "pass",
                    Some("fail") => "fail",
                    Some("inconclusive") => "inconclusive",
                    Some("skipped") => "skipped",
                    Some("not-run") => "not-run",
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
                        "reason": dimension.as_ref().map(|row| &row["reason"]).or_else(|| item.map(|item| &item["reason"])),
                        "measured": dimension.as_ref().map(|row| &row["domCssPx"]).or_else(|| item.map(|item| &item["measured"])),
                    }));
                }
                outcomes.push((
                    status,
                    check.kind.as_str(),
                    dimension
                        .as_ref()
                        .map(|row| &row["reason"])
                        .or_else(|| item.map(|item| &item["reason"]))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                ));
                let reason = dimension
                    .as_ref()
                    .map(|row| &row["reason"])
                    .or_else(|| item.map(|item| &item["reason"]))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                judged.push((status, check));
                if status == "fail" {
                    findings.push(finding(
                        screen,
                        viewport,
                        Some((index, check)),
                        failure_class(&check.kind, reason),
                        reason,
                        dimension
                            .as_ref()
                            .map(|row| &row["domCssPx"])
                            .or_else(|| item.map(|item| &item["measured"]))
                            .unwrap_or(&Value::Null),
                        reported.and_then(|s| s["screenshot"]["path"].as_str()),
                    ));
                }
                if status == "inconclusive"
                    && reason.contains("matched no element")
                    && let Some(node_id) = &check.node_id
                {
                    missing_nodes.push(json!({"screen": screen.id, "width": viewport.width,
                        "nodeId": node_id, "check": index, "kind": check.kind}));
                }
                if check.kind == "capture"
                    && status == "pass"
                    && let (Some(reference), Some(shot)) = (
                        &check.reference,
                        item.map(|item| &item["screenshot"])
                            .filter(|shot| shot.is_object()),
                    )
                {
                    match verify_capture(policy, &report_dir, shot, None) {
                        Ok(verified) => captures.push((screen, index, check, reference, verified)),
                        Err(Integrity::Tampered(issue)) => {
                            integrity_problems.push(json!({"screen": screen.id, "issue": issue}));
                        }
                        Err(Integrity::Unverified(issue)) => {
                            screenshot_issues.push(json!({"screen": screen.id, "issue": issue}));
                        }
                    }
                }
                if let Some(row) = dimension {
                    dimension_rows.push(row);
                }
            }
            let mut row = screen_row(
                screen,
                viewport,
                run.is_some(),
                reported,
                &environment_errors,
                &outcomes,
            );
            row["columns"] = columns(screen, viewport, &judged, &row);
            // A screen that never rendered fails as a runtime error, not as
            // the checks that could not measure it.
            if row["status"] == "fail" && !outcomes.iter().any(|(status, _, _)| *status == "fail") {
                let reason = array_text(&row["reasons"]);
                findings.push(finding(
                    screen,
                    viewport,
                    None,
                    "runtime-error",
                    &reason,
                    &Value::Null,
                    None,
                ));
            }
            screen_rows.push(row);
            let Some(shot) = reported
                .map(|screen| &screen["screenshot"])
                .filter(|shot| shot.is_object())
            else {
                screenshot_issues.push(
                    json!({"screen": screen.id, "issue": "not-verified: screenshot missing"}),
                );
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
        for (screen, index, check, reference, shot) in &captures {
            visual_comparisons.push(capture_comparison(
                screen, *index, check, reference, viewport, shot,
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
    let planned = planned_items(&plan);
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
        "findings": findings,
        "missingNodes": missing_nodes,
        "screenTable": screen_rows,
        "widthTable": plan.viewports.iter().map(|viewport| json!({
            "width": viewport.width, "height": viewport.height,
            "deviceScaleFactor": viewport.device_scale_factor,
            "dimensions": dimension_rows.iter().filter(|row| row["viewport"] == json!(viewport)).collect::<Vec<_>>()
        })).collect::<Vec<_>>(),
        "toleranceExceedances": dimension_rows.iter().filter(|row| row["status"] == "fail").collect::<Vec<_>>(),
        "dimensionCoverage": {"plannedRows": dimension_rows.len(), "returnedRows": dimension_rows.len(),
            "omittedRows": 0, "limit": MAX_DIMENSION_ROWS},
        "executor": report["executor"],
        "environment": report["environment"],
        "evidence": {
            "independentlyVerified": [
                "the plan file's content id and SHA-256",
                "the report's binding to that plan",
                "every count above, recomputed from the reported items",
                "each screenshot's SHA-256, byte length, PNG decoding and width",
                "dimension arithmetic and per-check tolerance filtering from reported CSS geometry; available retained Figma reference dimensions are rechecked",
            ],
            "executorDeclared": [
                "element geometry, locator counts, visibility and non-dimension check outcomes",
                "page, console and network errors",
                "timing, browser and environment versions",
            ],
            "notJudged": "Whether the screens look right to a person. Layout checks measure the relations the plan names and nothing else.",
        },
    }))
}

/// One screen at one width, folded from its checks, its own record and the
/// environment errors the run could pin on it. This is the row a person reads
/// to learn whether `/about` works at 390; `failures` and `runs` say the same
/// things but spread across three lists.
///
/// A page error is attributed only when its message names this screen's route
/// (`navigation-failed` does). An uncaught script error carries no route, so
/// it leaves every screen at that width inconclusive rather than failing the
/// one that may not have thrown it.
fn screen_row(
    screen: &ScreenInput,
    viewport: &ViewportInput,
    run_reported: bool,
    reported: Option<&Value>,
    environment_errors: &[Value],
    outcomes: &[(&str, &str, String)],
) -> Value {
    let message = |error: &Value| error["message"].as_str().unwrap_or_default().to_owned();
    let own_navigation = environment_errors
        .iter()
        .filter(|error| {
            error["kind"] == "navigation-failed"
                && screen.route_path.as_deref().is_some_and(|route| {
                    error["message"]
                        .as_str()
                        .is_some_and(|text| text.starts_with(&format!("{route}:")))
                })
        })
        .map(|error| format!("page error: {}", message(error)))
        .collect::<Vec<_>>();
    let unattributed = environment_errors
        .iter()
        .filter(|error| error["kind"] == "page-error")
        .map(|error| format!("page error during this width's run: {}", message(error)))
        .collect::<Vec<_>>();
    let failed_checks = outcomes
        .iter()
        .filter(|(status, _, _)| *status == "fail")
        .map(|(_, kind, reason)| match *kind {
            "no-horizontal-overflow" => format!("horizontal overflow: {reason}"),
            "view-transition" => format!("view transition: {reason}"),
            kind => format!("{kind}: {reason}"),
        })
        .collect::<Vec<_>>();
    let record_status = reported.and_then(|screen| screen["status"].as_str());
    let record_reason = reported
        .and_then(|screen| screen["reason"].as_str())
        .unwrap_or_default();
    let (status, reasons) = if !run_reported || reported.is_none() {
        (
            "not-run",
            vec!["no report item for this screen at this width".to_owned()],
        )
    } else if !own_navigation.is_empty() {
        ("fail", own_navigation)
    } else if record_status == Some("failed") {
        ("fail", vec![format!("screen not reached: {record_reason}")])
    } else if !failed_checks.is_empty() {
        ("fail", failed_checks)
    } else if record_status == Some("skipped") {
        ("not-run", vec![format!("skipped: {record_reason}")])
    } else if !unattributed.is_empty() {
        ("inconclusive", unattributed)
    } else if outcomes.is_empty() {
        (
            "inconclusive",
            vec!["reached, but the plan names no checks for this screen".to_owned()],
        )
    } else if outcomes.iter().all(|(status, _, _)| *status == "pass") {
        ("pass", Vec::new())
    } else {
        (
            "inconclusive",
            outcomes
                .iter()
                .filter(|(status, _, _)| *status != "pass")
                .map(|(status, kind, reason)| format!("{kind} {status}: {reason}"))
                .collect(),
        )
    };
    json!({
        "screen": screen.id,
        "routePath": screen.route_path,
        "width": viewport.width,
        "status": status,
        "reasons": reasons,
        "screenshot": reported.and_then(|screen| screen["screenshot"]["path"].as_str()),
    })
}

#[allow(clippy::too_many_arguments)]
fn dimension_row(
    screen: &ScreenInput,
    index: usize,
    check: &CheckInput,
    viewport: &ViewportInput,
    item: Option<&Value>,
    document: &Value,
    acquired: Option<&[Value]>,
    hash_bound: bool,
    environment_valid: bool,
) -> Value {
    let matches = |reference: &&Value| {
        reference["screen"] == screen.id
            && reference["check"] == index
            && reference["viewportWidth"] == viewport.width
    };
    let reference = document["dimensionReferences"]
        .as_array()
        .into_iter()
        .flatten()
        .find(matches);
    let verified = acquired
        .and_then(|refs| refs.iter().find(matches))
        .filter(|fresh| {
            fresh["status"] == "acquired" && reference.is_some_and(|saved| *fresh == saved)
        });
    let measured = item.map(|item| &item["measured"]);
    // Report files are untrusted: expose only bounded identity and numeric
    // geometry, rather than echoing arbitrary objects attached to a check.
    let mapping = check
        .figma_dimensions
        .iter()
        .find(|r| r.viewport_width == viewport.width);
    let reference_summary = mapping.map(|mapping| {
        json!({
            "artifactId": mapping.artifact_id, "nodeId": mapping.node_id,
            "status": if verified.is_some() { "acquired" } else { "unverified" },
            "width": reference.and_then(|r| r["width"].as_f64()),
            "height": reference.and_then(|r| r["height"].as_f64()),
            "units": "figma-logical-px",
            "artifactContentHash": verified.and_then(|r| r["artifactContentHash"].as_str()),
            "staleness": "retained snapshot; not compared with live Figma",
        })
    });
    let dom = measured.map(|m| {
        json!({"x": m["subject"]["x"].as_f64(), "y": m["subject"]["y"].as_f64(),
        "width": m["subject"]["width"].as_f64(), "height": m["subject"]["height"].as_f64()})
    });
    let mut row = json!({"screen": screen.id, "check": index, "label": check.label,
        "viewport": viewport, "subject": check.subject, "mapping": "caller-declared",
        "reference": reference_summary, "referenceVerifiedAgainstRetainedAcquisition": verified.is_some(),
        "domCssPx": dom, "domEvidence": "executor-declared",
        "tolerancePx": check.tolerance_px, "deltaPx": null, "status": "inconclusive",
        "reason": "Design reference or reported CSS geometry is unverified."});
    let declared_status = item.and_then(|item| item["status"].as_str());
    if declared_status.is_none() {
        row["status"] = json!("not-run");
        row["reason"] = json!("No measurement was reported for this planned check.");
    } else if declared_status == Some("skipped") {
        row["status"] = json!("skipped");
        row["reason"] = json!("The executor skipped this measurement.");
    } else if reference.is_none() {
        row["reason"] = json!(
            "No acquired Figma dimensions were explicitly mapped at this width; no interpolation was applied."
        );
    } else if verified.is_none() {
        row["reason"] = json!(
            "The saved reference could not be matched to the retained acquisition; acquire and plan again."
        );
    } else if !hash_bound || !environment_valid {
        row["reason"] =
            json!("The plan file hash, screen completion or runtime environment is unverified.");
    } else if matches!(declared_status, Some("pass" | "fail")) {
        let m = measured.unwrap();
        if m["units"] == "css-px"
            && m["subjectCount"] == 1
            && m["subjectVisible"] == true
            && let (Some(width), Some(height)) = (
                m["subject"]["width"].as_f64(),
                m["subject"]["height"].as_f64(),
            )
            && width.is_finite()
            && height.is_finite()
            && width >= 0.0
            && height >= 0.0
        {
            let expected = verified.unwrap();
            let dw = width - expected["width"].as_f64().unwrap();
            let dh = height - expected["height"].as_f64().unwrap();
            let tolerance = check.tolerance_px.unwrap();
            row["deltaPx"] = json!({"width": dw, "height": dh});
            row["status"] = json!(if dw.abs() <= tolerance && dh.abs() <= tolerance {
                "pass"
            } else {
                "fail"
            });
            row["reason"] = json!(
                "Compared acquired logical dimensions with declared DOM CSS pixels without DPR or screenshot scaling."
            );
            row["executorOutcomeMatches"] = json!(row["status"] == item.unwrap()["status"]);
        }
    }
    row
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
    verify_capture(policy, report_dir, shot, Some(viewport))
}

/// An element capture has the element's size, not the viewport's; only its
/// bytes and PNG decoding are checked.
fn verify_capture(
    policy: &OutputPolicy,
    report_dir: &Path,
    shot: &Value,
    viewport: Option<&ViewportInput>,
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
    if let Some(viewport) = viewport {
        let expected = (f64::from(viewport.width) * viewport.device_scale_factor).round() as u32;
        let minimum = (f64::from(viewport.height) * viewport.device_scale_factor).round() as u32;
        if width != expected || height < minimum {
            return Err(Integrity::Unverified(format!(
                "{path}: {width}x{height} is not {expected} wide and at least {minimum} tall; the capture does not show this viewport"
            )));
        }
    }
    Ok(VerifiedShot {
        path: target.display_path().display().to_string(),
        sha256: digest,
        width,
        height,
    })
}

/// The one place a finding's identity is made, so a later run of the same
/// check at the same width finds its earlier attempts.
fn comparison_id(screen: &ScreenInput, kind: &str, subject: &str, width: u32) -> String {
    format!("{}|{kind}|{subject}|{width}", screen.id)
}

fn diff_path(actual: &str, id: &str) -> String {
    let directory = Path::new(actual).parent().unwrap_or(Path::new(""));
    let name = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    directory
        .join("diff")
        .join(format!("{name}.png"))
        .display()
        .to_string()
}

fn array_text(value: &Value) -> String {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("; ")
}

/// What kind of problem a failed check is, so a fix starts in the right
/// place: a page that does not render, content wider than its box, a box in
/// the wrong place or of the wrong size, a state or a transition missing.
pub(crate) fn failure_class(kind: &str, reason: &str) -> &'static str {
    match kind {
        "page-health" => "runtime-error",
        "no-horizontal-overflow" | "no-clipped-text" => "overflow",
        "bounds" if reason.starts_with("position") => "layout-position",
        "bounds" | "dimensions" | "visible" => "layout-size",
        "contained" | "aligned" | "spans" => "layout-position",
        "interaction" => "interaction",
        "view-transition" => "view-transition",
        _ => "layout-position",
    }
}

/// A concrete first step for each class. The measured evidence names the
/// element where the executor reported one.
pub(crate) fn fix_hint(class: &str, kind: &str, measured: &Value) -> String {
    match class {
        "runtime-error" => "Open the URL at this width and read the first page or console error: an error-boundary text (\"This page couldn't load\", \"Application error\") means a component threw while rendering or a data fetch failed. Fix the throwing component or give the data a loading and error state, then re-run.".to_owned(),
        "overflow" if kind == "no-clipped-text" => "Text is cut off by a box with overflow hidden: let the box grow (drop the fixed height or width the Figma frame had), let the text wrap, or, when truncation is designed, use textOverflow ellipsis or lineClamp.".to_owned(),
        "overflow" => format!(
            "Content is wider than the viewport{}: give the element maxW=\"100%\", let its row wrap (flexWrap=\"wrap\") or shrink (minW={{0}}), and turn a fixed Figma width into a responsive value at this width.",
            measured["widest"].as_str().map(|widest| format!(" (widest: {widest})")).unwrap_or_default()
        ),
        "layout-position" => "The element sits in the wrong place relative to its parent: compare the parent's gap, padding and alignment (justifyContent, alignItems) with the Figma frame, check the order of its children (a card out of order fails here) and any absolute offsets.".to_owned(),
        "layout-size" => "The element has the wrong size: compare its width and height props with the Figma sizing (fixed, fill or hug), make fill stretch at widths that were not drawn, and check an image's aspect ratio and min/max sizes.".to_owned(),
        "asset-mismatch" => "The rendered asset differs from the Figma node's render. Likely causes, in order: a CSS mask on a multi-colour SVG paints one solid shape (render it with <Image src> instead; masks are for single-colour icons only); the wrong file at the src path; a crop or objectFit (cover vs contain) or a missing viewBox. Open the diff PNG to see which.".to_owned(),
        "section-visual" => "The section differs from its Figma render: open the diff PNG, find the highlighted area and fix the elements there; the usual causes are font size, weight or line height, a missing background or border, and spacing.".to_owned(),
        "page-visual" => "The page differs from the frame: fix the section and asset comparisons first, they localise the problem; a difference only the page shows is usually the page background, its padding or the spacing between sections.".to_owned(),
        "interaction" => "A link or button shows no hover or focus change: give it the interaction recipe for its role from the plan defaults (theme tokens only, at most two properties, the shared 150-250ms transition) and a visible _focusVisible outline.".to_owned(),
        "view-transition" => "No view transition ran on this navigation: wrap the routed content in React's <ViewTransition> (or add @view-transition { navigation: auto } for a cross-document transition) and navigate with the framework's Link so it is a client navigation.".to_owned(),
        _ => "Re-run the check and read its reason.".to_owned(),
    }
}

fn finding(
    screen: &ScreenInput,
    viewport: &ViewportInput,
    check: Option<(usize, &CheckInput)>,
    class: &str,
    reason: &str,
    measured: &Value,
    screenshot: Option<&str>,
) -> Value {
    let kind = check.map_or("screen", |(_, check)| check.kind.as_str());
    let subject = check
        .and_then(|(index, check)| {
            check
                .node_id
                .clone()
                .or_else(|| check.label.clone())
                .or_else(|| Some(format!("#{index}")))
        })
        .unwrap_or_else(|| "page".to_owned());
    json!({
        "id": comparison_id(screen, kind, &subject, viewport.width),
        "class": class,
        "screen": screen.id,
        "routePath": screen.route_path,
        "width": viewport.width,
        "check": check.map(|(index, _)| index),
        "kind": kind,
        "label": check.and_then(|(_, check)| check.label.clone()),
        "nodeId": check.and_then(|(_, check)| check.node_id.clone()),
        "codeFile": screen.component_path,
        "reason": reason,
        "fixHint": fix_hint(class, kind, measured),
        "evidence": {"screenshot": screenshot, "measured": measured},
    })
}

/// The `devup_visual_compare` call for one element capture against the
/// Figma node's own native render.
fn capture_comparison(
    screen: &ScreenInput,
    index: usize,
    check: &CheckInput,
    reference: &CaptureReferenceInput,
    viewport: &ViewportInput,
    shot: &VerifiedShot,
) -> Value {
    let class = if reference.scope == "asset" {
        "asset-mismatch"
    } else {
        "section-visual"
    };
    let subject = check.node_id.clone().unwrap_or_else(|| format!("#{index}"));
    let id = comparison_id(screen, &reference.scope, &subject, viewport.width);
    let threshold = reference
        .threshold
        .unwrap_or(if reference.scope == "asset" {
            ASSET_THRESHOLD
        } else {
            SECTION_THRESHOLD
        });
    let mut blockers = Vec::new();
    if (viewport.device_scale_factor - 1.0).abs() > f64::EPSILON {
        blockers.push(format!(
            "Captured at deviceScaleFactor {}; the native render is at scale 1.",
            viewport.device_scale_factor
        ));
    }
    json!({
        "id": id,
        "class": class,
        "screen": screen.id,
        "routePath": screen.route_path,
        "nodeId": check.node_id,
        "label": check.label,
        "check": index,
        "codeFile": screen.component_path,
        "viewport": viewport,
        "screenshot": {"path": shot.path, "sha256": shot.sha256, "width": shot.width, "height": shot.height},
        "call": {
            "tool": "devup_visual_compare",
            "arguments": {
                "actual": shot.path,
                "reference": {"artifactId": reference.artifact_id, "assetId": reference.asset_id},
                "threshold": threshold,
                "diffPath": diff_path(&shot.path, &id),
            },
        },
        "ifItFails": {"class": class, "fixHint": fix_hint(class, "capture", &Value::Null)},
        "blockers": blockers,
        "note": "A size mismatch with the native render is reported as dimensions, not pixels: fix the element's size (its bounds check says by how much) and compare again.",
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
    let frame = screen
        .figma
        .iter()
        .flat_map(|figma| &figma.frames)
        .find(|frame| frame.width == viewport.width);
    let node_id = frame.map_or(node_id, |frame| frame.node_id.as_str());
    let artifact = frame.and_then(|frame| frame.artifact_id.clone());
    if artifact.is_some() {
        must_add.retain(|field| *field != "reference.artifactId");
    }
    let id = comparison_id(screen, "page", node_id, viewport.width);
    let threshold = frame
        .and_then(|frame| frame.threshold)
        .unwrap_or(PAGE_THRESHOLD);
    json!({
        "id": id,
        "class": "page-visual",
        "screen": screen.id,
        "routePath": screen.route_path,
        "nodeId": node_id,
        "codeFile": screen.component_path,
        "viewport": viewport,
        "screenshot": {"path": shot.path, "sha256": shot.sha256, "width": shot.width, "height": shot.height},
        "call": {
            "tool": "devup_visual_compare",
            "arguments": {
                "actual": shot.path,
                "reference": {"artifactId": artifact.unwrap_or_else(|| format!("<artifactId of devup_figma_export for node {node_id} with outputs [\"referencePng\"]>"))},
                "threshold": threshold,
                "diffPath": diff_path(&shot.path, &id),
                "environment": environment,
            },
        },
        "ifItFails": {"class": "page-visual", "fixHint": fix_hint("page-visual", "", &Value::Null)},
        "consumerMustAdd": must_add,
        "blockers": blockers,
        "note": "Filled from the plan, the verified screenshot and the executor's report. Export the frame with outputs [\"referencePng\"] first and put its artifactId in reference; devup_visual_compare pins that reference itself. The fields in consumerMustAdd are the consumer's to state.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn dimension_fixture() -> (ArtifactStore, String) {
        use devup_mcp_figma::{CollectedPayload, CollectionRequest, CollectionScope, FigmaTarget};
        let store = ArtifactStore::default();
        let target = FigmaTarget {
            file_key: "synthetic".into(),
            node_id: Some("target".into()),
            branch_key: None,
        };
        let request = CollectionRequest::new(target.clone(), CollectionScope::Node);
        let payload: CollectedPayload = serde_json::from_value(json!({
            "target": target, "scope": "node", "metadata": {},
            "snapshot": {"fileKey": "synthetic", "version": "1", "roots": ["target"],
                "nodes": {"target": {"id": "target", "type": "FRAME", "fields": {"width": 100, "height": 80}},
                    "bounds-only": {"id": "bounds-only", "type": "FRAME", "fields": {"absoluteBoundingBox": {"width": 100, "height": 80}}},
                    "errored": {"id": "errored", "type": "FRAME", "fields": {"width": 100, "height": 80}, "fieldErrors": {"width": "unavailable"}}}, "diagnostics": []},
            "variables": null, "styles": null, "completeness": "resolved-values-only", "sourceVersion": "1"
        })).unwrap();
        let artifact = store
            .get_or_acquire(
                super::super::artifacts::ArtifactRequestKey::from_collection(&request),
                false,
                || async { Ok(payload) },
            )
            .await
            .unwrap();
        (store, artifact.artifact_id)
    }

    fn dimensions_plan(artifact: &str) -> PlanInput {
        let widths = [1440, 1100, 858, 390];
        let mappings = widths
            .iter()
            .map(
                |width| json!({"viewportWidth": width, "artifactId": artifact, "nodeId": "target"}),
            )
            .collect::<Vec<_>>();
        serde_json::from_value(json!({
            "baseUrl": "http://127.0.0.1:4173", "allowedOrigins": ["http://127.0.0.1:4173"],
            "viewports": widths.iter().map(|width| json!({"width": width, "height": 100, "deviceScaleFactor": 2})).collect::<Vec<_>>(),
            "screens": [{"id": "dimensions", "routePath": "/", "checks": [
                {"kind": "dimensions", "subject": {"testId": "target"}, "tolerancePx": 0.5, "figmaDimensions": mappings},
                {"kind": "dimensions", "subject": {"testId": "target"}, "tolerancePx": 0.49, "figmaDimensions": mappings}
            ]}]
        })).unwrap()
    }

    #[tokio::test]
    async fn acquired_dimensions_are_rechecked_and_tolerances_recomputed_at_all_widths() {
        let (dir, policy) = root();
        let (store, artifact) = dimension_fixture().await;
        let planned = create_plan(
            dimensions_plan(&artifact),
            &policy,
            Some("plan.json"),
            Some("runner"),
            &store,
        )
        .await
        .unwrap();
        assert_eq!(planned["template"]["files"].as_array().unwrap().len(), 4);
        assert_eq!(planned["template"]["executed"], false);
        assert_eq!(
            std::fs::read(dir.join("runner/run.mjs")).unwrap(),
            include_bytes!("../../../../harness/runtime-audit/run.mjs")
        );
        let plan_bytes = std::fs::read(dir.join("plan.json")).unwrap();
        let mut report = json!({"schemaVersion": 1, "kind": REPORT_KIND, "planId": planned["planId"],
            "planSha256": sha256(&plan_bytes), "status": "completed", "runs": []});
        for width in [1440_u32, 1100, 858, 390] {
            let bytes = png(width * 2, 200);
            let name = format!("{width}.png");
            std::fs::write(dir.join(&name), &bytes).unwrap();
            report["runs"].as_array_mut().unwrap().push(json!({
                "viewport": {"width": width, "height": 100, "deviceScaleFactor": 2}, "environmentErrors": [],
                "screens": [{"id": "dimensions", "status": "completed",
                    "checks": (0..2).map(|index| json!({"index": index, "status": "pass", "measured": {
                        "units": "css-px", "subjectCount": 1, "subjectVisible": true,
                        "subject": {"x": 0, "y": 0, "width": 100.5, "height": 80, "extra": "untrusted-subject-extension"}}})).collect::<Vec<_>>(),
                    "screenshot": {"path": name, "sha256": sha256(&bytes), "bytes": bytes.len()}}]
            }));
        }
        let write = |value: &Value| {
            std::fs::write(dir.join("report.json"), serde_json::to_vec(value).unwrap()).unwrap()
        };
        write(&report);
        let result = ingest_with_artifacts(&policy, "plan.json", "report.json", &store)
            .await
            .unwrap();
        assert_eq!(result["verdict"], "fail", "{result}");
        assert_eq!(result["summary"]["pass"], 4);
        assert_eq!(result["summary"]["fail"], 4);
        assert!(!result.to_string().contains("untrusted-subject-extension"));
        assert_eq!(result["toleranceExceedances"].as_array().unwrap().len(), 4);
        for row in result["widthTable"].as_array().unwrap() {
            assert_eq!(row["dimensions"][0]["deltaPx"]["width"], 0.5);
            assert_eq!(
                row["dimensions"][0]["referenceVerifiedAgainstRetainedAcquisition"],
                true
            );
            assert_eq!(row["dimensions"][1]["executorOutcomeMatches"], false);
        }

        let mut ambiguous = report.clone();
        let repeated = ambiguous["runs"][0]["screens"][0]["checks"][0].clone();
        ambiguous["runs"][0]["screens"][0]["checks"]
            .as_array_mut()
            .unwrap()
            .push(repeated);
        write(&ambiguous);
        assert_eq!(
            ingest_with_artifacts(&policy, "plan.json", "report.json", &store)
                .await
                .unwrap()["reason"],
            "ambiguous-report-items"
        );

        let mut missing = report.clone();
        missing["runs"].as_array_mut().unwrap().pop();
        missing["runs"][0]["screens"][0]["checks"][0]["status"] = json!("skipped");
        write(&missing);
        let result = ingest_with_artifacts(&policy, "plan.json", "report.json", &store)
            .await
            .unwrap();
        assert_eq!(result["verdict"], "inconclusive");
        assert_eq!(result["summary"]["notRun"], 2);
        assert_eq!(result["summary"]["skipped"], 1);
        assert_eq!(
            result["widthTable"][3]["dimensions"][0]["status"],
            "not-run"
        );

        write(&report);
        let empty_store = ArtifactStore::default();
        let result = ingest_with_artifacts(&policy, "plan.json", "report.json", &empty_store)
            .await
            .unwrap();
        assert_eq!(result["summary"]["inconclusive"], 8);
        assert_eq!(result["toleranceExceedances"], json!([]));
        assert_eq!(
            result["widthTable"][0]["dimensions"][0]["referenceVerifiedAgainstRetainedAcquisition"],
            false
        );

        for (field, value) in [
            ("width", json!(100.5)),
            ("height", json!(80.5)),
            ("status", json!("caller-asserted")),
            ("artifactContentHash", json!("forged")),
        ] {
            let mut forged = planned.clone();
            forged["dimensionReferences"][0][field] = value;
            let forged_bytes = serde_json::to_vec(&forged).unwrap();
            std::fs::write(dir.join("plan.json"), &forged_bytes).unwrap();
            let mut rebound = report.clone();
            rebound["planSha256"] = json!(sha256(&forged_bytes));
            write(&rebound);
            let result = ingest_with_artifacts(&policy, "plan.json", "report.json", &store)
                .await
                .unwrap();
            assert_eq!(
                result["widthTable"][0]["dimensions"][0]["status"], "inconclusive",
                "{field}"
            );
        }

        std::fs::write(dir.join("plan.json"), &plan_bytes).unwrap();
        write(&report);
        std::fs::write(dir.join("1440.png"), png(2880, 201)).unwrap();
        assert_eq!(
            ingest_with_artifacts(&policy, "plan.json", "report.json", &store)
                .await
                .unwrap()["reason"],
            "artifact-changed"
        );
        drop(policy);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn dimension_references_and_template_outputs_refuse_unverified_or_unsafe_inputs() {
        let (dir, policy) = root();
        let (store, artifact) = dimension_fixture().await;
        let input = dimensions_plan(&artifact);
        let mut oversized = input.clone();
        oversized.screens[0].checks =
            vec![input.screens[0].checks[0].clone(); MAX_DIMENSION_ROWS / 4 + 1];
        assert!(validate_plan(&oversized).is_err());
        let mut defaults = serde_json::to_value(&input).unwrap();
        defaults.as_object_mut().unwrap().remove("viewports");
        assert_eq!(
            serde_json::from_value::<PlanInput>(defaults)
                .unwrap()
                .viewports
                .iter()
                .map(|v| v.width)
                .collect::<Vec<_>>(),
            vec![1440, 1100, 858, 390]
        );
        for node_id in ["bounds-only", "errored", "absent"] {
            let mut missing = input.clone();
            missing.screens[0].checks[0].figma_dimensions[0].node_id = node_id.into();
            let value = create_plan(missing, &policy, None, None, &store)
                .await
                .unwrap();
            assert_eq!(
                value["dimensionReferences"][0]["status"], "unverified",
                "{node_id}"
            );
        }
        for tolerance in [-0.1, 10000.1] {
            let mut invalid = input.clone();
            invalid.screens[0].checks[0].tolerance_px = Some(tolerance);
            assert!(validate_plan(&invalid).is_err());
        }
        let mut duplicate = input.clone();
        let mapping = duplicate.screens[0].checks[0].figma_dimensions[0].clone();
        duplicate.screens[0].checks[0]
            .figma_dimensions
            .push(mapping);
        assert!(validate_plan(&duplicate).is_err());
        for (output, template) in [
            (Some("safe.json"), Some("../escape")),
            (Some("../escape.json"), Some("safe")),
            (None, Some("safe")),
        ] {
            assert!(
                create_plan(input.clone(), &policy, output, template, &store)
                    .await
                    .is_err()
            );
            assert!(!dir.join("safe.json").exists());
            assert!(!dir.join("safe/run.mjs").exists());
        }
        std::fs::create_dir_all(dir.join("existing")).unwrap();
        std::fs::write(dir.join("existing/checks.mjs"), b"keep").unwrap();
        assert!(
            create_plan(input, &policy, Some("safe.json"), Some("existing"), &store)
                .await
                .is_err()
        );
        assert_eq!(
            std::fs::read(dir.join("existing/checks.mjs")).unwrap(),
            b"keep"
        );
        assert!(!dir.join("safe.json").exists());
        assert!(!dir.join("existing/run.mjs").exists());
        drop(policy);
        std::fs::remove_dir_all(dir).unwrap();
    }

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
    fn a_view_transition_check_needs_the_link_to_follow() {
        let mut plan = plan_input();
        plan.screens[0].checks.push(
            serde_json::from_value(json!({"kind": "view-transition", "subject": {"testId": "go"}}))
                .unwrap(),
        );
        assert!(validate_plan(&plan).is_ok());
        plan.screens[0]
            .checks
            .push(serde_json::from_value(json!({"kind": "view-transition"})).unwrap());
        let error = validate_plan(&plan).unwrap_err();
        assert!(
            error
                .message
                .contains("view-transition check takes a subject"),
            "{}",
            error.message
        );
    }

    #[test]
    fn the_new_check_kinds_take_what_they_measure() {
        let ok = |check: Value| {
            let mut plan = plan_input();
            plan.screens[0]
                .checks
                .push(serde_json::from_value(check).unwrap());
            validate_plan(&plan).map_err(|error| error.message)
        };
        let node = |id: &str| json!({"css": format!("[data-figma-node-id=\"{id}\"]")});
        assert!(ok(json!({"kind": "page-health"})).is_ok());
        assert!(ok(json!({"kind": "no-clipped-text"})).is_ok());
        assert!(ok(json!({"kind": "interaction", "state": "hover"})).is_ok());
        assert!(
            ok(
                json!({"kind": "bounds", "subject": node("1:2"), "container": node("1:1"),
            "expected": {"x": 0, "y": 0, "width": 10, "height": 10}, "widths": [1440]})
            )
            .is_ok()
        );
        assert!(
            ok(json!({"kind": "capture", "subject": node("1:2"),
            "reference": {"scope": "asset", "artifactId": "a", "assetId": "b"}}))
            .is_ok()
        );
        assert!(
            ok(json!({"kind": "bounds", "subject": node("1:2"), "container": node("1:1")}))
                .unwrap_err()
                .contains("expected")
        );
        assert!(
            ok(json!({"kind": "interaction"}))
                .unwrap_err()
                .contains("hover or focus")
        );
        assert!(
            ok(json!({"kind": "capture", "subject": node("1:2")}))
                .unwrap_err()
                .contains("reference")
        );
        assert!(
            ok(json!({"kind": "visible", "subject": node("1:2"), "state": "hover"}))
                .unwrap_err()
                .contains("only valid for interaction")
        );
    }

    #[test]
    fn view_transition_checks_move_behind_the_rest_of_their_screen() {
        let mut plan = plan_input();
        plan.screens[0].checks = serde_json::from_value(json!([
            {"kind": "view-transition", "subject": {"testId": "go"}, "label": "vt"},
            {"kind": "page-health", "label": "health"},
            {"kind": "no-horizontal-overflow", "label": "overflow"},
        ]))
        .unwrap();
        order_checks(&mut plan);
        let labels = plan.screens[0]
            .checks
            .iter()
            .map(|check| check.label.clone().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(labels, ["health", "overflow", "vt"]);
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

    /// The screen table is what `devup_screen_plan status` reads: one row per
    /// screen and width, a failed overflow check named as overflow, a failed
    /// navigation pinned on its own route, and a width with no run not-run.
    #[test]
    fn the_screen_table_folds_checks_and_page_errors_per_width() {
        let (dir, policy) = root();
        let (id, plan_file) = write_plan(&dir, &policy);
        let mut report = report_for(&dir, &plan_file, &id, "pass", 800);
        report["runs"][0]["screens"][0]["checks"][0]["status"] = json!("fail");
        report["runs"][0]["screens"][0]["checks"][0]["reason"] = json!("content reaches 412px");
        report["runs"][1]["environmentErrors"] =
            json!([{"kind": "navigation-failed", "message": "/today: HTTP 500"}]);
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
        let row = |screen: &str, width: u32| {
            result["screenTable"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["screen"] == screen && row["width"] == width)
                .cloned()
                .unwrap()
        };
        assert_eq!(row("today", 360)["status"], "fail");
        assert_eq!(
            row("today", 360)["reasons"][0],
            "horizontal overflow: content reaches 412px"
        );
        assert_eq!(row("today", 1200)["status"], "fail");
        assert_eq!(
            row("today", 1200)["reasons"][0],
            "page error: /today: HTTP 500"
        );
        assert_eq!(row("next", 360)["status"], "pass");
        assert_eq!(row("next", 360)["screenshot"], "360-next.png");

        report["runs"].as_array_mut().unwrap().truncate(1);
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
        let missing = result["screenTable"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["width"] == 1200)
            .collect::<Vec<_>>();
        assert_eq!(missing.len(), 2);
        assert!(missing.iter().all(|row| row["status"] == "not-run"));
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

    /// Every failed check comes back as a finding of one class with the
    /// Figma node, the code file and a fix hint; a capture comes back as a
    /// ready asset comparison against the node's own render, and checks
    /// limited to a width are neither planned nor not-run elsewhere.
    #[test]
    fn failures_are_classified_and_captures_become_asset_comparisons() {
        let (dir, policy) = root();
        let node = |id: &str| json!({"css": format!("[data-figma-node-id=\"{id}\"]")});
        let input: PlanInput = serde_json::from_value(json!({
            "baseUrl": "http://127.0.0.1:4173", "allowedOrigins": ["http://127.0.0.1:4173"],
            "viewports": [{"width": 1440, "height": 900}, {"width": 390, "height": 900}],
            "screens": [{"id": "home", "routePath": "/", "componentPath": "src/app/page.tsx",
                "figma": {"nodeId": "1:1", "drawnWidths": [1440], "frames": [{"width": 1440, "nodeId": "1:1", "artifactId": "art-1"}]},
                "checks": [
                    {"kind": "page-health"},
                    {"kind": "bounds", "subject": node("1:3"), "container": node("1:2"), "nodeId": "1:3",
                        "expected": {"x": 0, "y": 0, "width": 10, "height": 10}, "widths": [1440]},
                    {"kind": "interaction", "state": "hover", "widths": [1440]},
                    {"kind": "capture", "subject": node("1:4"), "nodeId": "1:4", "widths": [1440],
                        "reference": {"scope": "asset", "artifactId": "art-1", "assetId": "icon"}},
                ]}],
        }))
        .unwrap();
        let written = plan(
            input,
            &policy,
            Some(&dir.join("plan.json").to_string_lossy()),
        )
        .unwrap();
        assert_eq!(
            written["expected"]["checkItems"], 5,
            "four at 1440, health alone at 390"
        );
        let plan_bytes = std::fs::read(dir.join("plan.json")).unwrap();
        let page = png(1440, 900);
        let icon = png(24, 24);
        std::fs::write(dir.join("page.png"), &page).unwrap();
        std::fs::write(dir.join("icon.png"), &icon).unwrap();
        let shot = |bytes: &[u8], path: &str| json!({"path": path, "sha256": sha256(bytes), "bytes": bytes.len()});
        let report = json!({"schemaVersion": 1, "kind": REPORT_KIND, "planId": written["planId"],
        "planSha256": sha256(&plan_bytes), "status": "completed", "runs": [
            {"viewport": {"width": 1440, "height": 900, "deviceScaleFactor": 1}, "environmentErrors": [],
             "screens": [{"id": "home", "status": "completed", "screenshot": shot(&page, "page.png"), "checks": [
                {"index": 0, "status": "pass", "reason": "ok", "measured": {}},
                {"index": 1, "status": "fail", "reason": "position: y off by 60px", "measured": {}},
                {"index": 2, "status": "fail", "reason": "hover changes nothing on 1 of 2", "measured": {}},
                {"index": 3, "status": "pass", "reason": "captured", "measured": {}, "screenshot": shot(&icon, "icon.png")},
             ]}]},
            {"viewport": {"width": 390, "height": 900, "deviceScaleFactor": 1}, "environmentErrors": [],
             "screens": [{"id": "home", "status": "completed", "checks": [
                {"index": 0, "status": "fail", "reason": "the page shows \"Application error\"", "measured": {}},
             ]}]},
        ]});
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
        assert_eq!(result["summary"]["notRun"], 0, "{result}");
        let findings = result["findings"].as_array().unwrap();
        let classes = findings
            .iter()
            .map(|finding| {
                (
                    finding["class"].as_str().unwrap(),
                    finding["width"].as_u64().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            classes,
            [
                ("layout-position", 1440),
                ("interaction", 1440),
                ("runtime-error", 390)
            ]
        );
        let position = &findings[0];
        assert_eq!(position["nodeId"], "1:3");
        assert_eq!(position["codeFile"], "src/app/page.tsx");
        assert_eq!(position["id"], "home|bounds|1:3|1440");
        assert!(
            position["fixHint"]
                .as_str()
                .unwrap()
                .contains("order of its children")
        );
        assert!(
            findings[2]["fixHint"]
                .as_str()
                .unwrap()
                .contains("error-boundary")
        );

        let comparisons = result["visualComparisons"].as_array().unwrap();
        let page_call = comparisons
            .iter()
            .find(|c| c["class"] == "page-visual")
            .unwrap();
        assert_eq!(
            page_call["call"]["arguments"]["reference"],
            json!({"artifactId": "art-1"})
        );
        assert_eq!(page_call["call"]["arguments"]["threshold"], PAGE_THRESHOLD);
        let asset_call = comparisons
            .iter()
            .find(|c| c["class"] == "asset-mismatch")
            .unwrap();
        assert_eq!(asset_call["id"], "home|asset|1:4|1440");
        assert_eq!(
            asset_call["call"]["arguments"]["reference"],
            json!({"artifactId": "art-1", "assetId": "icon"})
        );
        assert_eq!(
            asset_call["call"]["arguments"]["threshold"],
            ASSET_THRESHOLD
        );
        assert!(
            asset_call["call"]["arguments"]["diffPath"]
                .as_str()
                .unwrap()
                .ends_with("home_asset_1_4_1440.png")
        );
        assert!(
            asset_call["ifItFails"]["fixHint"]
                .as_str()
                .unwrap()
                .contains("multi-colour SVG")
        );
        let wide = result["screenTable"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["width"] == 1440)
            .unwrap();
        assert_eq!(wide["columns"]["geometry"], "fail");
        assert_eq!(wide["columns"]["asset"], "1 to compare");
        assert_eq!(wide["columns"]["page"], "1 to compare");
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
