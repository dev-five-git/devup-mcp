use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use devup_mcp_figma::{CollectedPayload, DiagnosticSeverity, FidelityImpact};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{artifacts::ArtifactStore, output::OutputPolicy, tools::VisualCompareInput};

/// Who observed a value or a result. Static source reading never proves what a
/// browser renders, so it is reported apart from browser observation.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceSource {
    #[default]
    Static,
    Browser,
    AutomatedTest,
    Manual,
}
impl EvidenceSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::Browser => "browser",
            Self::AutomatedTest => "automated-test",
            Self::Manual => "manual",
        }
    }
    fn observed(self) -> bool {
        !matches!(self, Self::Static)
    }
}

/// Measured style values for one state. Lengths are CSS logical pixels
/// (Figma logical units); position is relative to the same origin on both
/// sides (the screen frame). Colours accept #rgb, #rrggbb, #rrggbbaa, rgb() and
/// rgba(). A property absent on either side is not compared.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct StyleSnapshot {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub color: Option<String>,
    pub background_color: Option<String>,
    pub border_color: Option<String>,
    pub border_width: Option<f64>,
    pub border_radius: Option<f64>,
    pub font_family: Option<String>,
    pub font_size: Option<f64>,
    pub font_weight: Option<f64>,
    pub line_height: Option<f64>,
    pub letter_spacing: Option<f64>,
}

/// Why a verification cannot be finished inside the current request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum BlockerKind {
    /// The user must grant an access or run something the agent may not.
    Permission,
    /// An external environment (server, credentials, device, data) must change.
    Environment,
    /// A design or decision owned by someone else is missing.
    External,
    /// Tried and cannot be resolved in this request.
    Unresolvable,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct VerificationBlocker {
    pub kind: BlockerKind,
    /// The cause found so far.
    pub cause: String,
    /// The recommended resolution.
    pub recommendation: Option<String>,
    /// What the recommended resolution would let the agent verify.
    pub verifies: Option<String>,
    /// The action or permission needed from the user.
    pub user_action: Option<String>,
}

/// One state (default, hover, active, focus, selected, disabled, ...) of a
/// component or screen, with the Figma design and the implementation capture.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct StateCheck {
    /// Component file path or screen id the state belongs to; checks with the same target and routePath are grouped.
    pub target: String,
    pub route_path: Option<String>,
    pub state: String,
    /// Figma node drawing this state. With artifactId its size, position, colour, border and typography are read from the cached snapshot.
    pub figma_node_id: Option<String>,
    /// Design values; each overrides the value read from the snapshot.
    pub design: Option<StyleSnapshot>,
    /// Implementation values for this state.
    pub implementation: Option<StyleSnapshot>,
    /// Who measured `implementation`; default static (source reading).
    #[serde(default)]
    pub implementation_source: EvidenceSource,
    /// Pixel comparison through devup_visual_compare (same input), only when a visual comparison is needed.
    pub visual: Option<VisualCompareInput>,
    /// Geometry and typography tolerance in CSS px, default 0.5.
    #[schemars(range(min = 0.0, max = 8.0))]
    pub tolerance_px: Option<f64>,
    /// States this target must cover; missing ones are reported unverified.
    #[serde(default)]
    pub expected_states: Vec<String>,
    /// Fix or verification attempts already made, for the unresolved report.
    #[serde(default)]
    pub attempts: Vec<String>,
    pub blocker: Option<VerificationBlocker>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RequirementOrigin {
    /// A design specification card in Figma.
    DesignSpecCard,
    #[default]
    User,
    AcceptanceCriteria,
}

/// How a requirement can be proven. Interaction and visual requirements need
/// observed evidence; static source reading never proves them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RequirementCheck {
    #[default]
    Static,
    Visual,
    Interaction,
    Data,
    Content,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct RequirementTarget {
    pub route_path: Option<String>,
    pub component_path: Option<String>,
    pub state: Option<String>,
    pub figma_node_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceStatus {
    Pass,
    Fail,
    NotRun,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct RequirementEvidence {
    pub source: EvidenceSource,
    pub status: EvidenceStatus,
    pub detail: String,
    /// The screen this evidence was observed on; required to cover each screen of a multi-screen requirement.
    pub route_path: Option<String>,
    /// Test name, report path or diff path.
    pub reference: Option<String>,
}

/// A design spec card or user requirement linked to screens, components and states.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct TracedRequirement {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub origin: RequirementOrigin,
    /// The spec card node when origin is design-spec-card.
    pub figma_node_id: Option<String>,
    #[serde(default)]
    pub targets: Vec<RequirementTarget>,
    #[serde(default)]
    pub check: RequirementCheck,
    /// Set only when the requirement does not apply; the reason is reported.
    pub not_applicable_reason: Option<String>,
    #[serde(default)]
    pub evidence: Vec<RequirementEvidence>,
    #[serde(default)]
    pub attempts: Vec<String>,
    pub blocker: Option<VerificationBlocker>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DesignGapKind {
    MissingNode,
    MissingProperty,
    ExportFailed,
    ReadFailed,
}

/// A Figma node or property a read or export did not deliver.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct DesignGap {
    pub node_id: String,
    pub property: Option<String>,
    pub kind: DesignGapKind,
    pub cause: String,
}

/// One change to review, with the path the user follows to check it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct ReviewChange {
    pub id: Option<String>,
    pub summary: String,
    pub route_path: Option<String>,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub steps: Vec<String>,
    pub expected: Option<String>,
    #[serde(default)]
    pub requirement_ids: Vec<String>,
}

/// Everything the matrix reads besides the caller's own lists.
pub(super) struct MatrixContext<'a> {
    pub ui: &'a Value,
    pub root: &'a Path,
    pub payload: Option<&'a CollectedPayload>,
    /// The caller named an artifact that could not be read.
    pub artifact_unavailable: Option<String>,
    pub policy: Option<&'a OutputPolicy>,
    pub store: &'a ArtifactStore,
}

pub(super) struct MatrixInput<'a> {
    pub state_checks: &'a [StateCheck],
    pub requirements: &'a [TracedRequirement],
    pub design_gaps: &'a [DesignGap],
    pub changes: &'a [ReviewChange],
    pub component_path: Option<&'a str>,
}
impl MatrixInput<'_> {
    pub(super) fn is_empty(&self) -> bool {
        self.state_checks.is_empty()
            && self.requirements.is_empty()
            && self.design_gaps.is_empty()
            && self.changes.is_empty()
    }
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}
fn array(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn present(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|s| !s.is_empty())
}
fn normalize_path(path: &str) -> String {
    path.trim()
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_owned()
}

/// One spelling per state so `pressed` and `active` are one row.
pub(super) fn normalize_state(state: &str) -> String {
    let state = state.trim().to_ascii_lowercase().replace([' ', '_'], "-");
    match state.as_str() {
        "pressed" | "press" | "active" => "active".into(),
        "hovered" | "hover" | "mouse-over" => "hover".into(),
        "focused" | "focus" | "focus-visible" | "focusvisible" => "focus".into(),
        "enabled" | "normal" | "rest" | "idle" | "default" | "base" => "default".into(),
        "checked" | "selected" | "current" => "selected".into(),
        "disabled" => "disabled".into(),
        _ => state,
    }
}
