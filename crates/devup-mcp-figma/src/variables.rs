use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{DevupError, ErrorCode, ResourceKind, UpstreamResult, UsedResourceRefs};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceStyleRef {
    pub id: String,
    pub style_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer_start: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer_end: Option<usize>,
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
    #[serde(default)]
    pub unresolved: Vec<UnresolvedResource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnresolvedResource {
    pub id: String,
    pub kind: ResourceKind,
    pub reason: String,
}

pub(crate) struct UsedResourceMerge {
    pub result: UpstreamResult,
    pub unresolved: Vec<UnresolvedResource>,
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
) -> Result<UpstreamResult, DevupError> {
    let mut variables = Vec::new();
    let mut styles_by_id = std::collections::BTreeMap::<String, Value>::new();
    let mut consumer_counts = std::collections::BTreeMap::<String, usize>::new();
    let mut consumer_fragments =
        std::collections::BTreeMap::<String, std::collections::BTreeMap<usize, Vec<Value>>>::new();
    for batch in batches {
        variables.extend(batch.variables);
        for mut style in batch.styles {
            let Some(object) = style.as_object_mut() else {
                return Err(invalid_variable_result());
            };
            let Some(id) = object.get("id").and_then(Value::as_str).map(str::to_owned) else {
                return Err(invalid_variable_result());
            };
            if let Some(start) = object
                .remove("$consumerStart")
                .and_then(|value| value.as_u64())
            {
                let entries = object
                    .remove("$consumerEntries")
                    .and_then(|value| value.as_array().cloned())
                    .ok_or_else(invalid_variable_result)?;
                let consumers = entries
                    .into_iter()
                    .map(expand_consumer_entry)
                    .collect::<Result<Vec<_>, _>>()?;
                consumer_fragments
                    .entry(id)
                    .or_default()
                    .insert(start as usize, consumers);
                continue;
            }
            let count = object
                .remove("$consumerCount")
                .and_then(|value| value.as_u64())
                .unwrap_or(0) as usize;
            consumer_counts.insert(id.clone(), count);
            styles_by_id.insert(id, style);
        }
    }
    let mut styles = Vec::with_capacity(catalog.styles.len());
    for style_ref in &catalog.styles {
        let mut style = styles_by_id.remove(&style_ref.id).ok_or_else(|| {
            DevupError::new(
                ErrorCode::DevupFigmaVersionChanged,
                "수집 중 Figma style이 삭제되거나 변경되었습니다.",
                true,
            )
        })?;
        let expected = consumer_counts.remove(&style_ref.id).unwrap_or(0);
        let mut consumers = Vec::with_capacity(expected);
        for (start, mut fragment) in consumer_fragments.remove(&style_ref.id).unwrap_or_default() {
            if start != consumers.len() {
                return Err(incomplete_consumers());
            }
            consumers.append(&mut fragment);
        }
        if consumers.len() != expected {
            return Err(incomplete_consumers());
        }
        style
            .as_object_mut()
            .ok_or_else(invalid_variable_result)?
            .insert("consumers".to_owned(), Value::Array(consumers));
        styles.push(style);
    }
    Ok(UpstreamResult {
        raw: json!({
            "collections": catalog.collections,
            "variables": variables,
            "styles": styles,
            "usedRemoteVariables": [],
            "localComplete": catalog.local_complete,
            "usedRemoteComplete": catalog.used_remote_complete
        }),
    })
}

pub(crate) fn merge_used_resource_results(
    refs: &UsedResourceRefs,
    batches: impl IntoIterator<Item = VariableBatchResult>,
) -> Result<UsedResourceMerge, DevupError> {
    let mut variables_by_id = std::collections::BTreeMap::<String, Value>::new();
    let mut styles_by_id = std::collections::BTreeMap::<String, Value>::new();
    let mut unresolved_by_key =
        std::collections::BTreeMap::<(ResourceKind, String), UnresolvedResource>::new();

    for batch in batches {
        for variable in batch.variables {
            let id = resource_value_id(&variable)?;
            variables_by_id.insert(id, variable);
        }
        for style in batch.styles {
            let id = resource_value_id(&style)?;
            styles_by_id.insert(id, style);
        }
        for unresolved in batch.unresolved {
            unresolved_by_key.insert((unresolved.kind, unresolved.id.clone()), unresolved);
        }
    }

    let mut variables = Vec::with_capacity(refs.variable_ids.len());
    for id in &refs.variable_ids {
        if let Some(variable) = variables_by_id.remove(id) {
            variables.push(variable);
        } else {
            unresolved_by_key
                .entry((ResourceKind::Variable, id.clone()))
                .or_insert_with(|| UnresolvedResource {
                    id: id.clone(),
                    kind: ResourceKind::Variable,
                    reason: "notFoundOrUnavailable".to_owned(),
                });
        }
    }

    let mut styles = Vec::with_capacity(refs.styles.len());
    for style_ref in &refs.styles {
        if let Some(style) = styles_by_id.remove(&style_ref.id) {
            styles.push(style);
        } else {
            unresolved_by_key
                .entry((ResourceKind::Style, style_ref.id.clone()))
                .or_insert_with(|| UnresolvedResource {
                    id: style_ref.id.clone(),
                    kind: ResourceKind::Style,
                    reason: "notFoundOrUnavailable".to_owned(),
                });
        }
    }

    let unresolved = unresolved_by_key.into_values().collect::<Vec<_>>();
    let used_remote_variables = variables
        .iter()
        .filter(|variable| variable.get("remote").and_then(Value::as_bool) == Some(true))
        .cloned()
        .collect::<Vec<_>>();
    let used_remote_complete = unresolved.is_empty();
    Ok(UsedResourceMerge {
        result: UpstreamResult {
            raw: json!({
                "collections": [],
                "variables": variables,
                "styles": styles,
                "usedRemoteVariables": used_remote_variables,
                "localComplete": false,
                "usedRemoteComplete": used_remote_complete,
                "unresolved": &unresolved
            }),
        },
        unresolved,
    })
}

fn resource_value_id(value: &Value) -> Result<String, DevupError> {
    value
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(invalid_variable_result)
}

fn expand_consumer_entry(entry: Value) -> Result<Value, DevupError> {
    let values = entry.as_array().ok_or_else(invalid_variable_result)?;
    if values.len() != 3 {
        return Err(invalid_variable_result());
    }
    let node_id = values[0].as_str().ok_or_else(invalid_variable_result)?;
    let node_type = values[1].as_str().ok_or_else(invalid_variable_result)?;
    Ok(json!({
        "node": { "$nodeId": node_id, "$nodeType": node_type },
        "fields": values[2].clone()
    }))
}

fn incomplete_consumers() -> DevupError {
    DevupError::new(
        ErrorCode::DevupFigmaVersionChanged,
        "수집 중 Figma style consumer 목록이 변경되었습니다.",
        true,
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_compact_consumer_fragments_in_range_order() {
        let catalog = VariableCatalog {
            collections: Vec::new(),
            variable_ids: Vec::new(),
            styles: vec![ResourceStyleRef {
                id: "s1".to_owned(),
                style_type: "TEXT".to_owned(),
                consumer_start: None,
                consumer_end: None,
            }],
            local_complete: true,
            used_remote_complete: false,
        };
        let batches = vec![
            VariableBatchResult {
                variables: Vec::new(),
                styles: vec![json!({
                    "id": "s1",
                    "name": "Body",
                    "styleType": "TEXT",
                    "value": {"fontSize": 16},
                    "$consumerCount": 2
                })],
                unresolved: Vec::new(),
            },
            VariableBatchResult {
                variables: Vec::new(),
                styles: vec![json!({
                    "id": "s1",
                    "styleType": "TEXT",
                    "$consumerStart": 1,
                    "$consumerEntries": [["2:2", "TEXT", ["textStyleId"]]]
                })],
                unresolved: Vec::new(),
            },
            VariableBatchResult {
                variables: Vec::new(),
                styles: vec![json!({
                    "id": "s1",
                    "styleType": "TEXT",
                    "$consumerStart": 0,
                    "$consumerEntries": [["2:1", "TEXT", ["textStyleId"]]]
                })],
                unresolved: Vec::new(),
            },
        ];

        let merged = merge_variable_results(catalog, batches).unwrap();
        let consumers = merged.raw["styles"][0]["consumers"].as_array().unwrap();
        assert_eq!(consumers.len(), 2);
        assert_eq!(consumers[0]["node"]["$nodeId"], "2:1");
        assert_eq!(consumers[1]["node"]["$nodeId"], "2:2");
        assert_eq!(merged.raw["styles"][0]["value"]["fontSize"], 16);
    }
}
