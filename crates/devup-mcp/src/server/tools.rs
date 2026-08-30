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
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FigmaToJsonInput {
    pub url: String,
    #[serde(default = "default_scope")]
    pub scope: String,
    #[serde(default)]
    pub include_diagnostics: bool,
}

fn default_scope() -> String {
    "node".to_owned()
}
