use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `doctor` never touches OAuth state; it measures whether the direct
/// connection is usable right now and returns client-specific setup
/// guidance. `configure` persists a pre-registered client credential
/// (`clientId`, optional `clientSecret`) so later `login` calls skip Dynamic
/// Client Registration entirely; the secret is stored in the OS credential
/// store and never echoed back. See `server::diagnostics`.
/// `status` answers which skills the code devup-mcp emits needs and which of
/// them this workspace actually has. `install` writes the embedded ones.
///
/// An external skill is never installed by this tool: its publisher ships no
/// licence, so devup-mcp reports the command rather than the bytes, and running
/// that command is the caller's.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SkillsInput {
    #[serde(default = "default_skills_action")]
    #[schemars(extend("enum" = super::validation::SKILL_ACTIONS))]
    pub action: String,
    /// Which skills to install. Empty installs every embedded skill that is
    /// missing, which is the usual case on a machine that has just been set up.
    #[serde(default)]
    pub names: Vec<String>,
    /// The project whose skills are being reported on or installed into.
    ///
    /// Omitted, this falls back to the server's own write root, and that is
    /// only the project when the two were configured to be the same. A host
    /// that granted one shared parent - an Orca worktree pool, a monorepo
    /// checkout - made every call report on that parent instead, where no
    /// runtime looks for skills. Pass the directory the code is going into,
    /// the same one the other tools take.
    #[serde(default)]
    pub project_root: Option<String>,
}

fn default_skills_action() -> String {
    "status".to_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthInput {
    #[schemars(extend("enum" = super::validation::AUTH_ACTIONS))]
    pub action: String,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FigmaExportInput {
    /// Caller-declared observations from an independently inspected screenshot.
    /// Used only to rank actual explore candidates when selection is empty;
    /// no screenshot pixels are received or interpreted by this server.
    #[serde(default)]
    pub screenshot_observations: Option<super::screenshot_observations::ScreenshotObservations>,
    #[serde(flatten)]
    pub response_selection: super::compact::ResponseSelection,
    /// web-ready-images acquires every in-scope IMAGE full source as WebP q85,
    /// capped at display width*2 without upscaling, with asset-css-map.json and
    /// CSS effects, in sorted scheduled batches of at most six.
    /// Custom assetRequests must be exported separately to avoid preset overrides.
    #[serde(default)]
    #[schemars(extend("enum" = ["web-ready-images"]))]
    pub preset: Option<String>,
    /// The Figma link to export. Optional while exactly one Devup Bridge
    /// plugin is attached: omitted, or `figma-bridge://current`, it is the
    /// file that plugin has open and the node selected in Figma.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub artifact_id: Option<String>,
    #[serde(default = "default_outputs")]
    #[schemars(extend("items" = serde_json::json!({
        "type": "string",
        "enum": super::validation::EXPORT_OUTPUTS,
    })))]
    pub outputs: Vec<String>,
    /// full preserves the complete collected theme. used-only keeps tokens
    /// referenced by generated code and their dependencies. missing-only
    /// returns additions absent from projectRoot's existing devup.json and
    /// preserves existing names/values. The latter two require a code output.
    #[serde(default = "default_devup_json_mode")]
    #[schemars(extend("enum" = ["full", "used-only", "missing-only"]))]
    pub devup_json_mode: String,
    /// Saved designFingerprints sidecar JSON text. Request designChanges to compare
    /// captured converter inputs; this does not verify rendered pixels or code on disk.
    #[serde(default)]
    pub previous_design_fingerprints: Option<String>,
    #[serde(default)]
    pub component_name: Option<String>,
    #[serde(default)]
    pub include_diagnostics: bool,
    /// Opt in to data-figma-node-id attributes for runtime design audits.
    #[serde(default)]
    pub figma_node_ids: bool,
    #[serde(default = "default_scope")]
    #[schemars(extend("enum" = super::validation::COLLECTION_SCOPES))]
    pub scope: String,
    #[serde(default = "default_root_layout")]
    #[schemars(extend("enum" = super::validation::ROOT_LAYOUTS))]
    pub root_layout: String,
    /// Name every asset after the node it came from rather than after its
    /// layer, so two drawings a designer named alike get a file each, and a
    /// picture drawn at three widths gets one per width at that width's own
    /// size.
    ///
    /// On by default. Named after the layer, as the plugin names them, one
    /// file serves every node sharing that name: eight nodes on the notice
    /// screen claimed one file holding five different drawings, and a
    /// photograph drawn at three widths kept whichever width was exported
    /// last, so at the other two it was the wrong size for its box. Set it
    /// false for the plugin's own naming, which the code generator still uses
    /// by default when driven as a library.
    #[serde(default = "default_true")]
    pub asset_names_per_node: bool,
    /// Opens the two outputs that describe the design rather than the screen,
    /// `rawSnapshot` and `rawPayload`.
    ///
    /// For one question only: a screen looks wrong, and it has to be decided
    /// whether the generator is at fault or the design says so. Answering
    /// that means reading the collected design beside the generated code.
    ///
    /// Nothing that implements a screen needs them. Measured across ten real
    /// captured screens the tsx already carries 100% of the nodes, text,
    /// typography, assets and layout the design expects, and on one screen
    /// asking for them by habit spent about eight bytes for every one of code.
    #[serde(default)]
    pub debug: bool,
    /// Narrow rawSnapshot, rawPayload and sourceMap to these raw Figma property
    /// names (e.g. "fills", "fontSize", "itemSpacing") or groups: "colors",
    /// "spacing", "typography", "shadow", "radius", "size". Node identity and
    /// tree links are always kept. Use it so a diagnostic answer fits in one
    /// response instead of being cut off.
    #[serde(default)]
    pub fields: Vec<String>,
    #[serde(default)]
    pub strict: bool,
    #[serde(default)]
    pub refresh: bool,
    #[serde(default)]
    /// pageScaffold (or frame:<nodeId>:pageScaffold) names a project directory;
    /// other output keys name files. Scaffold writes require pageScaffold.write=true.
    /// designFingerprints writes a versioned JSON sidecar and replaces an existing file
    /// through the guarded output transaction, unless scaffold options prohibit it.
    pub output_paths: BTreeMap<String, String>,
    #[serde(default)]
    pub page_scaffold: Option<super::projection::page_scaffold::PageScaffoldOptions>,
    #[serde(default)]
    pub frame_ids: Vec<String>,
    #[serde(default)]
    pub all_screens: bool,
    /// Recommend 1–3 assets per call, maximum 6. Slow calls return an assetJob for status/resume.
    /// assetManifest is automatically added to outputs with a warning when omitted.
    #[serde(default)]
    #[schemars(extend("maxItems" = 6))]
    pub asset_requests: Vec<FigmaAssetRequestInput>,
    #[serde(default = "default_delivery")]
    #[schemars(extend("enum" = super::validation::DELIVERY_MODES))]
    pub delivery: String,
    /// The project the generated TSX is going into, so the export can say
    /// whether it fits.
    ///
    /// Tokens are named after the Figma variables and text styles a screen
    /// actually uses. That is right for a project with no `devup.json` yet,
    /// and wrong for one that already has its own names: a Braillify Studio
    /// login screen came back asking for `$background`, `$innerBg`,
    /// `$kakaoLogo`, `$title`, `$textSub`, `$borderLight` and the typography
    /// `mainSubText`, `titleSmMed`, `bodyMed`, of which that project defines
    /// none — it calls them `$bg`, `$panel`, `$text`, `$textSubtle`,
    /// `$borderStrong`, `body`, `bodyL`. Every one of those is an error
    /// `devup_ui_validate` reports against the same project, so the export
    /// handed back code this server's own validator refuses.
    ///
    /// Given a root, `projectThemeValidation` carries that verdict in the
    /// export itself, naming the tokens that do not exist and what the
    /// project calls the ones it has. Omitted, nothing is read from disk and
    /// the response is unchanged.
    ///
    /// Pass it on whichever call returns the outputs. A collection of any
    /// size answers `in_progress` first and hands the TSX back from the
    /// `jobId` poll, so that poll needs the root too.
    #[serde(default)]
    pub project_root: Option<String>,
    /// Opt in to output that keeps anchors when the screen is not at its drawn
    /// width. Omitted, the export is unchanged and exact at the drawn width
    /// only; `widthBehavior` then reports what would move. With it the root
    /// takes its container's width and each positioned node keeps the anchor
    /// named in `anchors` (from `widthBehavior` candidates), else the anchor of
    /// an anchored container it is drawn inside, else its Figma constraint.
    /// Single-width screens only: a capture with several widths uses
    /// responsiveTsx.
    #[serde(default)]
    pub width_policy: Option<WidthPolicyInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct FigmaMeasureInput {
    #[serde(default)]
    pub url: Option<String>,
    /// Explicit 1..100 distinct node IDs. Instance descendant IDs are supported.
    #[schemars(extend("minItems" = 1, "maxItems" = 100))]
    pub node_ids: Vec<String>,
    /// Optional caller-declared screenshot pixel dimensions of this measured node.
    /// A uniform scale is calibrated from actual node dimensions; UI DPR is not used.
    #[serde(default)]
    pub calibration: Option<MeasureCalibration>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct MeasureCalibration {
    pub node_id: String,
    pub screen_width_px: f64,
    pub screen_height_px: f64,
}

fn default_devup_json_mode() -> String {
    "full".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct WidthPolicyInput {
    /// "fluid": the root takes its container's width.
    #[schemars(extend("enum" = ["fluid"]))]
    pub root: String,
    /// nodeId -> left | right | center | stretch | scale.
    #[serde(default)]
    #[schemars(extend("additionalProperties" = serde_json::json!({
        "type": "string",
        "enum": ["left", "right", "center", "stretch", "scale"],
    })))]
    pub anchors: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FigmaAssetRequestInput {
    /// Caller-declared use-site aspect ratio for audit comparison. Browser
    /// measurements remain unverified; omitted means actual usage unknown.
    #[serde(default)]
    pub usage_aspect_ratio: Option<f64>,
    /// WebP RGB quality, 0..100, default 85. In lossless mode controls compression
    /// effort, preserving decoded RGBA including transparent RGB. WebP only.
    #[serde(default)]
    #[schemars(range(min = 0, max = 100))]
    pub quality: Option<u8>,
    /// WebP only; default false. Alpha is preserved in both modes.
    #[serde(default)]
    pub lossless: Option<bool>,
    /// original-source (default) acquires the full IMAGE source with effects
    /// applied by CSS. node explicitly renders that node; visible renders a
    /// selected in-scope ancestor subtree with outside-scope context unverified.
    /// Legacy fill requires matching codec and scale 1, with no resizing.
    #[serde(default = "default_asset_render_mode")]
    #[schemars(extend("enum" = ["original-source", "fill", "node", "visible", "auto"]))]
    pub render_mode: String,
    /// Select a discovered asset, or supply nodeId for any acquired node.
    #[serde(default)]
    pub asset_id: String,
    #[serde(default)]
    pub node_id: Option<String>,
    /// A proven in-scope ancestor (or the requested node itself) for visible.
    #[serde(default)]
    pub visible_scope_node_id: Option<String>,
    /// Optional no-upscale WebP width cap; original-source only.
    #[serde(default)]
    #[schemars(range(min = 1, max = 8192))]
    pub max_width: Option<u32>,
    /// Emit name.webp (1x) and name@2x.webp, capped at source resolution.
    #[serde(default)]
    pub srcset: bool,
    /// Omitted: raster image fills (original-source) become WebP q85, anything else PNG.
    #[serde(default)]
    #[schemars(extend("enum" = super::validation::ASSET_FORMATS))]
    pub format: Option<String>,
    #[serde(default = "default_asset_scale")]
    #[schemars(extend("exclusiveMinimum" = 0, "maximum" = 4))]
    pub scale: f64,
    #[serde(default)]
    pub output_path: Option<String>,
}

fn default_asset_render_mode() -> String {
    "original-source".to_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FigmaSearchInput {
    /// The Figma link to search in. Optional while exactly one Devup Bridge
    /// plugin is attached: omitted, the whole file that plugin has open.
    #[serde(default)]
    pub url: Option<String>,
    pub query: String,
    #[serde(default)]
    pub node_types: Vec<String>,
    #[serde(default = "default_match", rename = "match")]
    #[schemars(extend("enum" = super::validation::SEARCH_MATCH_KINDS))]
    pub match_kind: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FigmaExploreInput {
    #[serde(default)]
    pub screenshot_observations: Option<super::screenshot_observations::ScreenshotObservations>,
    /// The Figma link of the anchor node. Optional while exactly one Devup
    /// Bridge plugin is attached: omitted, the node selected in Figma.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default = "default_explore_limit")]
    pub limit: usize,
    #[serde(default = "default_true")]
    pub include_text_preview: bool,
    #[serde(default)]
    pub refresh: bool,
}

/// `scope` is `theme` (project `devup.json` tokens), `api` (project
/// `openapi.json` endpoints/schemas), `db` (Vespertide `models/*.json`
/// tables/columns), or `all`. Reads whichever target file(s) actually
/// exist on disk at call time — never cached across calls, never inferred
/// when missing. See `server::project_context`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectContextInput {
    /// theme | api | db | ui | routes | all. UI/routes are opt-in and excluded from all. Routes reuse the authoritative Next App Router UI inventory. UI filter: literal case-sensitive substring of component path, exported/local name, route path or page path. Reports caps and unresolved evidence.
    #[schemars(extend("enum" = super::validation::PROJECT_CONTEXT_SCOPES.into_iter().chain(["ui"]).collect::<Vec<_>>()))]
    pub scope: String,
    #[serde(default)]
    pub project_root: Option<String>,
    #[serde(default)]
    /// Literal case-sensitive substring match, not a regular expression. Example: primary matches primaryBg but not Primary; .* matches only the literal characters .*. Theme: token names; API: path, lowercase method, operationId and schema name; DB: table name.
    pub filter: Option<String>,
}

/// Validates devup-ui TSX against the rules in `server::project_context`'s
/// sibling module `ui_validate` (crate `devup-mcp-devup-ui`): unknown
/// `$token` references, hardcoded colors/lengths with an existing token,
/// unknown props on known primitives, and non-literal values inside
/// `css`/`globalCss`/`keyframes` calls. `strict: true` additionally fails
/// `ok` on warning-severity violations.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UiValidateInput {
    /// Copy outputResults.tsx.sectionCoverage (or a Section frame's equivalent)
    /// from devup_figma_export to check that captured sections/assets remain.
    /// This is static JSX identity coverage, not proof of runtime visibility.
    #[serde(default)]
    pub section_coverage: Option<SectionCoverageInput>,
    /// Actual acquisition policies keyed by assetId, including public assetPath
    /// and effectsBaked. Missing baking evidence remains unverified.
    #[serde(default)]
    #[schemars(with = "std::collections::BTreeMap<String, serde_json::Value>", extend("additionalProperties" = serde_json::json!({
        "type":"object",
        "description":"Actual asset acquisition evidence; omitted effectsBaked remains unknown.",
        "properties":{
            "renderMode":{"type":"string","enum":["original-source","fill","node","visible","auto"]},
            "effectsBaked":{"type":["boolean","null"]},
            "assetPath":{"type":["string","null"]},
            "sourcePixelSize":{"type":["object","null"],"properties":{"width":{"type":"number","exclusiveMinimum":0},"height":{"type":"number","exclusiveMinimum":0}}},
            "outputPixelSize":{"type":["object","null"],"properties":{"width":{"type":"number","exclusiveMinimum":0},"height":{"type":"number","exclusiveMinimum":0}}},
            "nodeBox":{"type":["object","null"],"properties":{"width":{"type":"number","exclusiveMinimum":0},"height":{"type":"number","exclusiveMinimum":0}}},
            "captureNodeId":{"type":["string","null"]},
            "captureBox":{"type":["object","null"],"properties":{"width":{"type":"number","exclusiveMinimum":0},"height":{"type":"number","exclusiveMinimum":0}}},
            "contextScope":{"type":["string","null"]},
            "variants":{"type":"array","items":{"type":"object","required":["path","width","height","density"],"properties":{"path":{"type":"string"},"width":{"type":"integer","minimum":1},"height":{"type":"integer","minimum":1},"density":{"type":"number","exclusiveMinimum":0}}}}
        }
    })))]
    pub asset_policies: BTreeMap<String, devup_mcp_devup_ui::codegen::AssetRepresentationPolicy>,
    /// Optional caller-supplied browser HTML (max 2 MiB); reports anchor href counts separately from static source coverage. Never fetches a URL or executes HTML.
    #[serde(default)]
    pub rendered_html: Option<String>,
    #[serde(default)]
    pub tsx: String,
    /// Optional supplied-source bundle: at most 64 entries and 1 MiB total
    /// UTF-8 path/content bytes. Enables structural checks without disk reads.
    /// Entries require path and content strings; include devup.json for tokens.
    /// Omit tsx when supplying files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Vec<std::collections::BTreeMap<String, String>>>", extend(
        "maxItems" = 64,
        "items" = serde_json::json!({"type":"object","required":["path","content"],"additionalProperties":{"not":{}},"properties":{"path":{"type":"string"},"content":{"type":"string"}}})
    ))]
    pub files: Option<Vec<devup_mcp_devup_ui::ui_validate::SourceFile>>,
    /// Optional source label echoed in diagnostics; never read as a file path.
    #[serde(default)]
    pub source_name: Option<String>,
    #[serde(default)]
    pub project_root: Option<String>,
    #[serde(default)]
    pub strict: bool,
    /// The `sourceMap` devup_figma_export returned with this tsx. Violations
    /// are joined to its entries on `elementIndex`, naming the Figma node and
    /// field the offending attribute was generated from. Not joined with
    /// files, which number elements per file.
    #[serde(default)]
    pub source_map: Option<SourceMapInput>,
    /// The width in px the tsx was written for, used by `layoutRisks` where a
    /// parent states no px width of its own.
    #[serde(default)]
    pub reference_width: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SectionCoverageInput {
    pub node_ids: Vec<String>,
    /// Asset references in the original generated output, including masks and backgrounds.
    #[serde(default)]
    pub asset_urls: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourceMapInput {
    /// A Section frame's map (`frames[].sourceMap`).
    #[serde(default)]
    pub entries: Vec<SourceMapEntryInput>,
    /// A single screen's map (`sourceMap`), or `sourceMap.byOutput.tsx`.
    #[serde(default)]
    pub tsx: Vec<SourceMapEntryInput>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourceMapEntryInput {
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub property: Option<String>,
    #[serde(default)]
    pub generated_property: Option<String>,
    #[serde(default)]
    pub resolution: Option<String>,
    #[serde(default)]
    pub element_index: Option<usize>,
}

/// `layers` selects which cross-layer drift checks to run
/// (`db-entity`, `entity-route`, `route-openapi`, `openapi-client`);
/// omitted or empty runs all four. See `server::stack_diff`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StackDiffInput {
    #[serde(default)]
    pub project_root: Option<String>,
    #[serde(default)]
    #[schemars(extend("items" = serde_json::json!({
        "type": "string",
        "enum": super::validation::STACK_DIFF_LAYERS,
    })))]
    pub layers: Vec<String>,
}

fn default_scope() -> String {
    "node".to_owned()
}

fn default_root_layout() -> String {
    "standalone".to_owned()
}

fn default_delivery() -> String {
    "auto".to_owned()
}

fn default_match() -> String {
    "normalized".to_owned()
}

fn default_limit() -> usize {
    20
}

fn default_explore_limit() -> usize {
    50
}

fn default_true() -> bool {
    true
}

/// `tsx` alone.
///
/// `devupJson` used to come with it, and measured against real use it was
/// paid for and then not used: both Figma tickets in the September report
/// received a `devupJson` and still called `devup_project_context` to read
/// the tokens they had to match. That is the correct order - a project
/// that already has a `devup.json` must match *that* file, and the theme
/// read out of Figma is a different set of names - so the default was
/// answering a question nobody asks while enlarging every response.
fn default_outputs() -> Vec<String> {
    vec!["tsx".to_owned()]
}

fn default_asset_scale() -> f64 {
    2.0
}

/// Compare a PNG; this tool never renders or runs commands.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct VisualCompareInput {
    pub actual: String,
    pub reference: super::visual_compare::VisualReference,
    /// Changed-pixel ratio in [0, 1]; omitted uses 0.005 (0.5 percent).
    #[serde(default)]
    #[schemars(range(min = 0.0, max = 1.0))]
    pub threshold: Option<f64>,
    #[serde(default)]
    pub environment: Option<super::visual_compare::VisualEnvironment>,
    /// Include the red diff PNG using the standard binary delivery limits.
    #[serde(default)]
    pub include_diff: bool,
    /// Also write the diff PNG to this path under an allowed root, as the
    /// evidence an audit finding cites.
    #[serde(default)]
    pub diff_path: Option<String>,
    #[serde(default = "default_delivery")]
    #[schemars(extend("enum" = super::validation::DELIVERY_MODES))]
    pub delivery: String,
}

/// Read-only cross-layer slice. Anchors are literal identifiers, never inferred
/// from requirement or acceptanceCriteria. A path-only API anchor is invalid.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct FeatureTraceInput {
    /// Explicit Figma destination node ID -> code route/URL bindings. No route is inferred from a Figma node ID or name.
    #[serde(default)]
    pub navigation_targets: BTreeMap<String, String>,
    pub project_root: Option<String>,
    pub route_path: Option<String>,
    pub figma_node_id: Option<String>,
    pub artifact_id: Option<String>,
    pub operation_id: Option<String>,
    pub api_path: Option<String>,
    pub method: Option<String>,
    pub component_path: Option<String>,
    pub table_name: Option<String>,
    pub requirement: Option<String>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    /// Caller-supplied componentTsx export tied to the explicit Figma anchor.
    /// Its provenance is caller-declared; cached artifact generation is preferred.
    pub component_tsx: Option<String>,
    /// Per-array response cap, default 100, maximum 200. Byte cap is 65536.
    #[schemars(range(min = 1, max = 200))]
    pub max_items: Option<usize>,
}
