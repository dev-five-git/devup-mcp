use serde::Deserialize;
use serde_json::Value;

use crate::{DevupError, ErrorCode, UpstreamResult};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataDocument {
    pub file_key: String,
    #[serde(default)]
    pub version: Option<String>,
    pub root_id: String,
    #[serde(default)]
    pub nodes: Vec<MetadataNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default)]
    pub children_ids: Vec<String>,
    #[serde(default)]
    pub descendant_count: usize,
}

impl MetadataDocument {
    pub fn root(&self) -> Option<&MetadataNode> {
        self.nodes.iter().find(|node| node.id == self.root_id)
    }
}

pub fn metadata_from_result(result: &UpstreamResult) -> Result<MetadataDocument, DevupError> {
    find_metadata(&result.raw).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupSnapshotUnsupported,
            "Figma MCP 응답에서 metadata를 찾지 못했습니다.",
            false,
        )
    })
}

fn find_metadata(value: &Value) -> Option<MetadataDocument> {
    if let Ok(document) = serde_json::from_value::<MetadataDocument>(value.clone())
        && !document.file_key.is_empty()
        && !document.root_id.is_empty()
    {
        return Some(document);
    }
    match value {
        Value::Object(object) => {
            if let Some(Value::String(text)) = object.get("text")
                && let Ok(parsed) = serde_json::from_str::<Value>(text)
                && let Some(document) = find_metadata(&parsed)
            {
                return Some(document);
            }
            object.values().find_map(find_metadata)
        }
        Value::Array(values) => values.iter().find_map(find_metadata),
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|parsed| find_metadata(&parsed)),
        _ => None,
    }
}
