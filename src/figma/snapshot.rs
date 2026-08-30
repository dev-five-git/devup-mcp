use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{DevupError, ErrorCode, UpstreamResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default)]
    pub fields: Map<String, Value>,
    #[serde(default)]
    pub extra: Map<String, Value>,
    #[serde(default)]
    pub field_errors: BTreeMap<String, String>,
}

impl RawNode {
    pub fn typed_view(&self) -> TypedNode<'_> {
        TypedNode { node: self }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TypedNode<'a> {
    node: &'a RawNode,
}

impl<'a> TypedNode<'a> {
    pub fn id(&self) -> &'a str {
        &self.node.id
    }

    pub fn node_type(&self) -> &'a str {
        &self.node.node_type
    }

    pub fn value(&self, field: &str) -> Option<&'a Value> {
        self.node
            .fields
            .get(field)
            .or_else(|| self.node.extra.get(field))
    }

    pub fn string(&self, field: &str) -> Option<&'a str> {
        self.value(field).and_then(Value::as_str)
    }

    pub fn number(&self, field: &str) -> Option<f64> {
        self.value(field).and_then(Value::as_f64)
    }

    pub fn bool(&self, field: &str) -> Option<bool> {
        self.value(field).and_then(Value::as_bool)
    }

    pub fn name(&self) -> Option<&'a str> {
        self.string("name")
    }

    pub fn child_ids(&self) -> impl Iterator<Item = &'a str> {
        self.value("childrenIds")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotChunk {
    pub file_key: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub root_ids: Vec<String>,
    #[serde(default)]
    pub nodes: Vec<RawNode>,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub file_key: String,
    pub version: Option<String>,
    pub roots: Vec<String>,
    pub nodes: BTreeMap<String, RawNode>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn merge_chunks(chunks: Vec<SnapshotChunk>) -> Result<Snapshot, DevupError> {
    let first = chunks.first().ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupSnapshotUnsupported,
            "병합할 Figma snapshot이 없습니다.",
            false,
        )
    })?;
    let file_key = first.file_key.clone();
    let version = first.version.clone();
    let mut roots = Vec::new();
    let mut root_set = BTreeSet::new();
    let mut nodes = BTreeMap::new();
    let mut diagnostics = Vec::new();

    for chunk in chunks {
        if chunk.file_key != file_key || chunk.version != version {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaVersionChanged,
                "수집 중 Figma 파일 버전이 변경되었습니다. 다시 시도하세요.",
                true,
            ));
        }
        for root in chunk.root_ids {
            if root_set.insert(root.clone()) {
                roots.push(root);
            }
        }
        for node in chunk.nodes {
            if let Some(existing) = nodes.get(&node.id) {
                if existing != &node {
                    return Err(DevupError::new(
                        ErrorCode::DevupSnapshotUnsupported,
                        "동일한 Figma node에 서로 다른 snapshot 데이터가 반환되었습니다.",
                        true,
                    ));
                }
            } else {
                nodes.insert(node.id.clone(), node);
            }
        }
        diagnostics.extend(chunk.diagnostics);
    }

    Ok(Snapshot {
        file_key,
        version,
        roots,
        nodes,
        diagnostics,
    })
}

pub fn snapshot_chunk_from_result(result: &UpstreamResult) -> Result<SnapshotChunk, DevupError> {
    find_snapshot(&result.raw).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupSnapshotUnsupported,
            "Figma MCP 응답에서 snapshot 데이터를 찾지 못했습니다.",
            false,
        )
    })
}

fn find_snapshot(value: &Value) -> Option<SnapshotChunk> {
    if let Ok(snapshot) = serde_json::from_value::<SnapshotChunk>(value.clone())
        && !snapshot.file_key.is_empty()
        && !snapshot.nodes.is_empty()
    {
        return Some(snapshot);
    }

    match value {
        Value::Object(object) => {
            if let Some(Value::String(text)) = object.get("text")
                && let Some(snapshot) = parse_snapshot_text(text)
            {
                return Some(snapshot);
            }
            object.values().find_map(find_snapshot)
        }
        Value::Array(values) => values.iter().find_map(find_snapshot),
        Value::String(text) => parse_snapshot_text(text),
        _ => None,
    }
}

fn parse_snapshot_text(text: &str) -> Option<SnapshotChunk> {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|value| find_snapshot(&value))
}
