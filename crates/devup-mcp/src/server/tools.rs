use rmcp::schemars::JsonSchema;
use schemars::{Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `action` is `status`, `login`, `logout`, `configure`, or `doctor`.
/// `doctor` never touches OAuth state; it measures which connection paths
/// (direct OAuth, host handoff) are currently usable
/// and returns client-specific setup guidance. `configure` persists a
/// pre-registered client credential (`clientId`, optional `clientSecret`)
/// so later `login` calls skip Dynamic Client Registration entirely; the
/// secret is stored in the OS credential store and never echoed back. See
/// `server::diagnostics`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthInput {
    pub action: String,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FigmaToUiInput {
    pub url: String,
    #[serde(default)]
    pub component_name: Option<String>,
    #[serde(default)]
    pub include_diagnostics: bool,
    #[serde(default = "default_source_policy")]
    pub source_policy: String,
    #[serde(default = "default_scope")]
    pub scope: String,
    #[serde(default = "default_root_layout")]
    pub root_layout: String,
    #[serde(default)]
    pub output_path: Option<String>,
    #[serde(default = "default_delivery")]
    pub delivery: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FigmaToJsonInput {
    pub url: String,
    #[serde(default = "default_scope")]
    pub scope: String,
    #[serde(default)]
    pub include_diagnostics: bool,
    #[serde(default = "default_source_policy")]
    pub source_policy: String,
    #[serde(default)]
    pub output_path: Option<String>,
    #[serde(default = "default_delivery")]
    pub delivery: String,
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
    #[serde(default = "default_source_policy")]
    pub source_policy: String,
    #[serde(default = "default_scope")]
    pub scope: String,
    #[serde(default = "default_root_layout")]
    pub root_layout: String,
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
    #[serde(default)]
    pub asset_requests: Vec<FigmaAssetRequestInput>,
    #[serde(default = "default_delivery")]
    pub delivery: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FigmaAssetRequestInput {
    pub asset_id: String,
    #[serde(default = "default_asset_format")]
    #[schemars(extend("enum" = ["png", "jpg", "svg", "pdf"]))]
    pub format: String,
    #[serde(default = "default_asset_scale")]
    pub scale: u8,
    #[serde(default)]
    pub output_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContinueInput {
    pub session_id: String,
    pub call_id: String,
    // The verbatim result of a host-executed official Figma MCP read call.
    // Its shape is dictated by that upstream tool (text, image content
    // blocks, nested objects, ...), so the runtime type must stay
    // `serde_json::Value` and accept anything.
    //
    // schemars' blanket `JsonSchema` impl for `Value` maps this to the
    // JSON Schema 2020-12 boolean schema `true` ("accept anything"). That
    // is spec-legal, but several MCP clients' schema converters assume
    // every `properties` entry is a JSON object and reject a boolean value
    // outright, which discards the *entire* `tools/list` response, not
    // just this tool. `any_json_value_schema` overrides the generated
    // schema to keep the identical "accept any JSON" semantics while
    // expressing it as the empty object schema `{}`, which every JSON
    // Schema 2020-12 consumer can parse.
    //
    // NOTE: intentionally a plain `//` comment, not `///`: a doc comment
    // here would be captured by schemars as this field's schema
    // "description" and shipped over the wire on every tools/list call.
    #[schemars(schema_with = "any_json_value_schema")]
    pub result: serde_json::Value,
}

// See `ContinueInput::result` above for why this exists instead of relying
// on `serde_json::Value`'s default (boolean) schema. Plain comment for the
// same reason: schemars would otherwise turn `///` into this function's
// stand-in schema "description".
fn any_json_value_schema(_generator: &mut SchemaGenerator) -> Schema {
    json_schema!({})
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FigmaSearchInput {
    pub url: String,
    pub query: String,
    #[serde(default)]
    pub node_types: Vec<String>,
    #[serde(default = "default_match", rename = "match")]
    pub match_kind: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default = "default_source_policy")]
    pub source_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FigmaExploreInput {
    pub url: String,
    #[serde(default = "default_explore_limit")]
    pub limit: usize,
    #[serde(default = "default_true")]
    pub include_text_preview: bool,
    #[serde(default = "default_source_policy")]
    pub source_policy: String,
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
    pub scope: String,
    #[serde(default)]
    pub project_root: Option<String>,
    #[serde(default)]
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
    pub layers: Vec<String>,
}

fn default_scope() -> String {
    "node".to_owned()
}

fn default_root_layout() -> String {
    "standalone".to_owned()
}

fn default_source_policy() -> String {
    "auto".to_owned()
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

fn default_outputs() -> Vec<String> {
    vec!["tsx".to_owned(), "devupJson".to_owned()]
}

fn default_asset_format() -> String {
    "png".to_owned()
}

fn default_asset_scale() -> u8 {
    1
}
