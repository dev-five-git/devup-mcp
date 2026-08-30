use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{DevupError, ErrorCode, UpstreamResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceStyleRef {
    pub id: String,
    pub style_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceBatch {
    #[serde(default)]
    pub variable_ids: Vec<String>,
    #[serde(default)]
    pub styles: Vec<ResourceStyleRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VariableCatalog {
    #[serde(default)]
    pub collections: Vec<Value>,
    #[serde(default)]
    pub variable_ids: Vec<String>,
    #[serde(default)]
    pub styles: Vec<ResourceStyleRef>,
    #[serde(default)]
    pub local_complete: bool,
    #[serde(default)]
    pub used_remote_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VariableBatchResult {
    #[serde(default)]
    pub variables: Vec<Value>,
    #[serde(default)]
    pub styles: Vec<Value>,
}

pub(crate) fn catalog_from_result(result: &UpstreamResult) -> Result<VariableCatalog, DevupError> {
    find::<VariableCatalog>(&result.raw, &["collections", "variableIds", "styles"])
        .ok_or_else(invalid_variable_result)
}

pub(crate) fn batch_from_result(
    result: &UpstreamResult,
) -> Result<VariableBatchResult, DevupError> {
    find::<VariableBatchResult>(&result.raw, &["variables", "styles"])
        .ok_or_else(invalid_variable_result)
}

pub(crate) fn merge_variable_results(
    catalog: VariableCatalog,
    batches: impl IntoIterator<Item = VariableBatchResult>,
) -> UpstreamResult {
    let mut variables = Vec::new();
    let mut styles = Vec::new();
    for batch in batches {
        variables.extend(batch.variables);
        styles.extend(batch.styles);
    }
    UpstreamResult {
        raw: json!({
            "collections": catalog.collections,
            "variables": variables,
            "styles": styles,
            "usedRemoteVariables": [],
            "localComplete": catalog.local_complete,
            "usedRemoteComplete": catalog.used_remote_complete
        }),
    }
}

fn find<T>(value: &Value, required_keys: &[&str]) -> Option<T>
where
    T: for<'de> Deserialize<'de>,
{
    if let Value::Object(object) = value
        && required_keys.iter().all(|key| object.contains_key(*key))
        && let Ok(parsed) = serde_json::from_value(value.clone())
    {
        return Some(parsed);
    }
    match value {
        Value::Object(object) => {
            if let Some(Value::String(text)) = object.get("text")
                && let Ok(value) = serde_json::from_str::<Value>(text)
                && let Some(parsed) = find(&value, required_keys)
            {
                return Some(parsed);
            }
            object.values().find_map(|value| find(value, required_keys))
        }
        Value::Array(values) => values.iter().find_map(|value| find(value, required_keys)),
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|value| find(&value, required_keys)),
        _ => None,
    }
}

fn invalid_variable_result() -> DevupError {
    DevupError::new(
        ErrorCode::DevupThemeConflict,
        "Figma MCP 응답에서 변수/style batch를 찾지 못했습니다.",
        false,
    )
}
