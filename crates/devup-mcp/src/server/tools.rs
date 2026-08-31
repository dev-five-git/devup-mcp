use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthInput {
    pub action: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FigmaExportInput {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub artifact_id: Option<String>,
    #[serde(default = "default_outputs")]
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
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContinueInput {
    pub session_id: String,
    pub call_id: String,
    pub result: serde_json::Value,
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
