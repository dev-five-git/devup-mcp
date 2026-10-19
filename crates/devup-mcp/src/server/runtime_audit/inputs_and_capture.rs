
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
    /// text-align: subject computed text alignment matches edge from Figma.
    /// sequence-order: numbered siblings increase in visual reading order.
    /// section-layout: visible sections span at least half their container.
    /// no-clipped-content: internal clipped rows require an explicit carousel with controls.
    /// page-health: no error page text ("This page couldn't load",
    /// "Application error", ...), page error, console error or failed
    /// navigation while the screen was open.
    /// bounds: subject's size and position relative to container's top-left,
    /// in CSS px, against `expected` (the Figma geometry). Exact: layout is
    /// converted statically, so only half a pixel of rounding is allowed and
    /// no tolerance can be set.
    /// hidden: subject is absent or has no visible box - an element a width's
    /// frame does not draw.
    /// no-clipped-text: no text inside subject (or the page) is cut off by an
    /// overflow-hidden box; ellipsis and line clamp are intended and pass.
    /// interaction: hovering or keyboard-focusing subject (`state`) changes
    /// something visible; without a subject, every visible link and button.
    /// capture: screenshot subject's element box for devup_visual_compare
    /// against `reference`; ingest turns it into a ready call.
    /// no-low-contrast-text: text against effective computed background colours.
    /// image-loaded: every img and CSS background URL decodes with naturalWidth > 0.
    /// Both accept an optional subject scope.
    /// rendered-completeness: at one drawn width, the page is at least
    /// minHeightRatio of the frame's height, every listed section and image
    /// renders (images decoded), and no text is invisible against what is
    /// painted behind it. A drawn width without a passing one is not-run.
    #[schemars(extend("enum" = ["no-horizontal-overflow", "contained", "aligned", "spans", "visible", "dimensions", "view-transition", "page-health", "bounds", "no-clipped-text", "interaction", "capture", "hidden", "rendered-completeness", "sequence-order", "section-layout", "text-align", "no-clipped-content", "no-low-contrast-text", "image-loaded"]))]
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
    /// rendered-completeness: what the frame drawn at this width says must be
    /// on the page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completeness: Option<CompletenessInput>,
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
        "no-horizontal-overflow" | "page-health" | "no-clipped-content" => needs(false, false)?,
        "visible" | "view-transition" | "hidden" => needs(true, false)?,
        "no-clipped-text" | "no-low-contrast-text" | "image-loaded" => {
            if check.container.is_some() {
                return Err(invalid(format!("A {} check takes no container.", check.kind)));
            }
        }
        "bounds" => {
            needs(true, true)?;
            if check.tolerance_px.is_some() {
                return Err(invalid(
                    "A bounds check is exact (half a pixel of rounding); it takes no tolerancePx. A mismatch is code drift or a converter bug, never noise.",
                ));
            }
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
        "text-align" => {
            needs(true, false)?;
            if !matches!(check.edge.as_deref(), Some("left" | "right" | "center" | "justify")) {
                return Err(invalid("A text-align check needs edge left, right, center or justify."));
            }
        }
        "sequence-order" => needs(false, false)?,
        "section-layout" => {
            if check.subject.is_some() { needs(true, true)?; } else { needs(false, false)?; }
        }
        "rendered-completeness" => {
            needs(false, false)?;
            let Some(completeness) = &check.completeness else {
                return Err(invalid(
                    "A rendered-completeness check needs completeness {frameHeight, sections, images}.",
                ));
            };
            validate_completeness(completeness)?;
            if check.widths.len() != 1 {
                return Err(invalid(
                    "A rendered-completeness check holds one frame, so it names exactly one width.",
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
        (
            "completeness",
            check.completeness.is_some(),
            "rendered-completeness",
        ),
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
        fold(&["page-health", "no-horizontal-overflow", "no-clipped-content", "no-clipped-text", "no-low-contrast-text", "image-loaded"])
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
    let completeness = match fold(&["rendered-completeness"]) {
        "-" if drawn => "not-run",
        "-" => "not drawn",
        other => other,
    };
    json!({
        "health": health,
        "completeness": completeness,
        "geometry": fold(&["text-align", "sequence-order", "section-layout", "text-align", "bounds", "dimensions", "contained", "aligned", "spans", "visible"]),
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