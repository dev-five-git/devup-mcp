use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `doctor` never touches OAuth state; it measures whether the direct
/// connection is usable right now and returns client-specific setup
/// guidance. `configure` persists a pre-registered client credential
/// (`clientId`, optional `clientSecret`) so later `login` calls skip Dynamic
/// Client Registration entirely; the secret is stored in the OS credential
/// store and never echoed back. See `server::diagnostics`.
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
    #[serde(default)]
    pub component_name: Option<String>,
    #[serde(default)]
    pub include_diagnostics: bool,
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
    #[serde(default)]
    pub strict: bool,
    #[serde(default)]
    pub refresh: bool,
    #[serde(default)]
    pub output_paths: BTreeMap<String, String>,
    #[serde(default)]
    pub frame_ids: Vec<String>,
    #[serde(default)]
    pub all_screens: bool,
    /// Recommend 1–3 assets per call, maximum 6. Slow calls return an assetJob for status/resume.
    #[serde(default)]
    #[schemars(extend("maxItems" = 6))]
    pub asset_requests: Vec<FigmaAssetRequestInput>,
    #[serde(default = "default_delivery")]
    #[schemars(extend("enum" = super::validation::DELIVERY_MODES))]
    pub delivery: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FigmaAssetRequestInput {
    pub asset_id: String,
    #[serde(default = "default_asset_format")]
    #[schemars(extend("enum" = super::validation::ASSET_FORMATS))]
    pub format: String,
    #[serde(default = "default_asset_scale")]
    pub scale: u8,
    #[serde(default)]
    pub output_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FigmaSearchInput {
    pub url: String,
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
    pub url: String,
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
    #[schemars(extend("enum" = super::validation::PROJECT_CONTEXT_SCOPES))]
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
    pub tsx: String,
    /// Optional source label echoed in diagnostics; never read as a file path.
    #[serde(default)]
    pub source_name: Option<String>,
    #[serde(default)]
    pub project_root: Option<String>,
    #[serde(default)]
    pub strict: bool,
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

fn default_asset_format() -> String {
    "png".to_owned()
}

fn default_asset_scale() -> u8 {
    1
}
