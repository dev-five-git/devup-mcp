use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContinueInput {
    pub session_id: String,
    pub call_id: String,
    pub result: serde_json::Value,
}

fn default_scope() -> String {
    "node".to_owned()
}

fn default_source_policy() -> String {
    "auto".to_owned()
}
