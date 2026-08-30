use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::{
    CollectedParts, CollectionScope, DevupError, FigmaTarget, Snapshot, UpstreamResult,
    merge_chunks,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PayloadCompleteness {
    FullLocalPlusUsedRemote,
    UsedTokens,
    ResolvedValuesOnly,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectedPayload {
    pub target: FigmaTarget,
    pub scope: CollectionScope,
    pub metadata: Value,
    pub snapshot: Snapshot,
    pub variables: Option<UpstreamResult>,
    pub styles: Option<UpstreamResult>,
    pub completeness: PayloadCompleteness,
    pub source_version: Option<String>,
}

impl TryFrom<CollectedParts> for CollectedPayload {
    type Error = DevupError;

    fn try_from(parts: CollectedParts) -> Result<Self, Self::Error> {
        let snapshot = merge_chunks(parts.snapshot_chunks)?;
        let completeness = if parts.variables.is_some() {
            PayloadCompleteness::UsedTokens
        } else {
            PayloadCompleteness::ResolvedValuesOnly
        };
        Ok(Self {
            target: parts.target,
            scope: parts.scope,
            metadata: parts.metadata,
            snapshot,
            variables: parts.variables,
            styles: parts.styles,
            completeness,
            source_version: parts.source_version,
        })
    }
}

pub fn validate_payload_context(
    payload: &CollectedPayload,
    expected_target: &FigmaTarget,
) -> Result<(), DevupError> {
    if &payload.target != expected_target
        || payload.snapshot.file_key != expected_target.file_key
        || expected_target
            .node_id
            .as_ref()
            .is_some_and(|node_id| !payload.snapshot.roots.iter().any(|root| root == node_id))
    {
        return Err(DevupError::new(
            crate::ErrorCode::DevupFigmaHandoffInvalid,
            "Figma payload가 요청한 파일 또는 node와 일치하지 않습니다.",
            false,
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayloadStructure {
    pub metadata_shape: Value,
    pub variable_shape: Option<Value>,
    pub style_shape: Option<Value>,
    pub node_fields: BTreeMap<String, BTreeMap<String, Value>>,
    pub node_count: usize,
    pub field_error_count: usize,
    pub schema_hash: String,
}

impl PayloadStructure {
    pub fn from_payload(payload: &CollectedPayload) -> Self {
        let mut node_fields: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();
        let mut field_error_count = 0;
        for node in payload.snapshot.nodes.values() {
            let fields = node_fields.entry(node.node_type.clone()).or_default();
            for (name, value) in node.fields.iter().chain(&node.extra) {
                fields.insert(name.clone(), json_shape(value));
            }
            for name in node.field_errors.keys() {
                fields.insert(name.clone(), json!("error"));
                field_error_count += 1;
            }
        }
        let metadata_shape = json_shape(&payload.metadata);
        let variable_shape = payload
            .variables
            .as_ref()
            .map(|result| json_shape(&result.raw));
        let style_shape = payload
            .styles
            .as_ref()
            .map(|result| json_shape(&result.raw));
        let schema = json!({
            "metadata": &metadata_shape,
            "variables": &variable_shape,
            "styles": &style_shape,
            "nodeFields": &node_fields,
        });
        let schema_hash = sha256_hex(serde_json::to_vec(&schema).unwrap_or_default());
        Self {
            metadata_shape,
            variable_shape,
            style_shape,
            node_fields,
            node_count: payload.snapshot.nodes.len(),
            field_error_count,
            schema_hash,
        }
    }
}

fn json_shape(value: &Value) -> Value {
    match value {
        Value::Null => json!("null"),
        Value::Bool(_) => json!("boolean"),
        Value::Number(_) => json!("number"),
        Value::String(_) => json!("string"),
        Value::Array(values) => {
            let unique = values
                .iter()
                .map(json_shape)
                .map(|shape| serde_json::to_string(&shape).unwrap_or_default())
                .collect::<BTreeSet<_>>();
            json!({ "array": unique })
        }
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(name, value)| (name.clone(), json_shape(value)))
                .collect::<Map<_, _>>(),
        ),
    }
}

fn sha256_hex(bytes: Vec<u8>) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
