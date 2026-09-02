use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ResourceStyleRef, SnapshotChunk};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResourceScope {
    #[default]
    None,
    Used,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResourceKind {
    Variable,
    Style,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceOccurrence {
    pub node_id: String,
    pub field: String,
    pub resource_id: String,
    pub resource_kind: ResourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_type: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsedResourceRefs {
    pub variable_ids: Vec<String>,
    pub styles: Vec<ResourceStyleRef>,
    pub occurrences: Vec<ResourceOccurrence>,
}

pub fn collect_used_resource_refs(chunks: &[SnapshotChunk]) -> UsedResourceRefs {
    let mut variable_ids = BTreeSet::new();
    let mut styles = BTreeMap::<String, String>::new();
    let mut occurrences = BTreeSet::new();

    for chunk in chunks {
        for node in &chunk.nodes {
            for (field, value) in node.fields.iter().chain(&node.extra) {
                scan_value(
                    &node.id,
                    field,
                    value,
                    &mut variable_ids,
                    &mut styles,
                    &mut occurrences,
                );
            }
        }
    }

    UsedResourceRefs {
        variable_ids: variable_ids.into_iter().collect(),
        styles: styles
            .into_iter()
            .map(|(id, style_type)| ResourceStyleRef {
                id,
                style_type,
                consumer_start: None,
                consumer_end: None,
            })
            .collect(),
        occurrences: occurrences.into_iter().collect(),
    }
}

fn scan_value(
    node_id: &str,
    path: &str,
    value: &Value,
    variable_ids: &mut BTreeSet<String>,
    styles: &mut BTreeMap<String, String>,
    occurrences: &mut BTreeSet<ResourceOccurrence>,
) {
    match value {
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("VARIABLE_ALIAS")
                && let Some(id) = object.get("id").and_then(Value::as_str)
                && is_resource_id(id)
            {
                variable_ids.insert(id.to_owned());
                occurrences.insert(ResourceOccurrence {
                    node_id: node_id.to_owned(),
                    field: path.to_owned(),
                    resource_id: id.to_owned(),
                    resource_kind: ResourceKind::Variable,
                    style_type: None,
                });
            }

            for (field, child) in object {
                let child_path = format!("{path}.{field}");
                if let Some(style_type) = style_type_for_field(field)
                    && let Some(id) = child.as_str()
                    && is_resource_id(id)
                {
                    styles
                        .entry(id.to_owned())
                        .or_insert_with(|| style_type.to_owned());
                    occurrences.insert(ResourceOccurrence {
                        node_id: node_id.to_owned(),
                        field: child_path.clone(),
                        resource_id: id.to_owned(),
                        resource_kind: ResourceKind::Style,
                        style_type: Some(style_type.to_owned()),
                    });
                }
                scan_value(
                    node_id,
                    &child_path,
                    child,
                    variable_ids,
                    styles,
                    occurrences,
                );
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                scan_value(
                    node_id,
                    &format!("{path}[{index}]"),
                    child,
                    variable_ids,
                    styles,
                    occurrences,
                );
            }
        }
        Value::String(id) => {
            if let Some(field) = path.rsplit(['.', '[']).next()
                && let Some(style_type) = style_type_for_field(field)
                && is_resource_id(id)
            {
                styles
                    .entry(id.to_owned())
                    .or_insert_with(|| style_type.to_owned());
                occurrences.insert(ResourceOccurrence {
                    node_id: node_id.to_owned(),
                    field: path.to_owned(),
                    resource_id: id.to_owned(),
                    resource_kind: ResourceKind::Style,
                    style_type: Some(style_type.to_owned()),
                });
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn style_type_for_field(field: &str) -> Option<&'static str> {
    match field {
        "textStyleId" => Some("TEXT"),
        "fillStyleId" | "strokeStyleId" | "backgroundStyleId" => Some("PAINT"),
        "effectStyleId" => Some("EFFECT"),
        "gridStyleId" => Some("GRID"),
        _ => None,
    }
}

fn is_resource_id(id: &str) -> bool {
    !id.is_empty() && id != "figma.mixed" && id != "MIXED"
}
