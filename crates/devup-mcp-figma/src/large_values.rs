use std::collections::BTreeMap;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{DevupError, ErrorCode, SnapshotChunk, UpstreamResult};

pub const MAX_LARGE_VALUE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_LARGE_VALUE_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeValueCursor {
    pub next_offset: usize,
    pub max_chunk_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeValueDescriptor {
    pub node_id: String,
    pub field: String,
    pub byte_length: usize,
    pub sha256: String,
    pub cursor: LargeValueCursor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeValueReadOptions {
    pub node_id: String,
    pub field: String,
    pub offset: usize,
    pub max_chunk_bytes: usize,
    pub byte_length: usize,
    pub sha256: String,
    pub version: Option<String>,
}

impl LargeValueReadOptions {
    pub fn from_descriptor(
        descriptor: &LargeValueDescriptor,
        version: Option<String>,
        offset: usize,
    ) -> Self {
        Self {
            node_id: descriptor.node_id.clone(),
            field: descriptor.field.clone(),
            offset,
            max_chunk_bytes: descriptor
                .cursor
                .max_chunk_bytes
                .clamp(1, MAX_LARGE_VALUE_CHUNK_BYTES),
            byte_length: descriptor.byte_length,
            sha256: descriptor.sha256.clone(),
            version,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeValueFragment {
    pub file_key: String,
    pub version: Option<String>,
    pub node_id: String,
    pub field: String,
    pub offset: usize,
    pub next_offset: usize,
    pub byte_length: usize,
    pub sha256: String,
    pub data_base64: String,
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeValueUnsupported {
    pub file_key: String,
    pub version: Option<String>,
    pub node_id: String,
    pub field: String,
    pub byte_length: usize,
    pub sha256: String,
    pub error_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LargeValueResult {
    Fragment(LargeValueFragment),
    Unsupported(LargeValueUnsupported),
}

#[derive(Debug, Clone)]
pub struct LargeValueAssembler {
    expected_file_key: String,
    expected_version: Option<String>,
    descriptor: LargeValueDescriptor,
    fragments: BTreeMap<usize, Vec<u8>>,
    saw_complete: bool,
}

impl LargeValueAssembler {
    pub fn new(
        file_key: impl Into<String>,
        version: Option<String>,
        descriptor: LargeValueDescriptor,
    ) -> Result<Self, DevupError> {
        validate_descriptor(&descriptor)?;
        Ok(Self {
            expected_file_key: file_key.into(),
            expected_version: version,
            descriptor,
            fragments: BTreeMap::new(),
            saw_complete: false,
        })
    }

    pub fn descriptor(&self) -> &LargeValueDescriptor {
        &self.descriptor
    }

    pub fn push(&mut self, fragment: LargeValueFragment) -> Result<(), DevupError> {
        if fragment.file_key != self.expected_file_key
            || fragment.version != self.expected_version
            || fragment.node_id != self.descriptor.node_id
            || fragment.field != self.descriptor.field
            || fragment.byte_length != self.descriptor.byte_length
            || fragment.sha256 != self.descriptor.sha256
        {
            return Err(invalid(
                "large value fragment target or version does not match the request.",
            ));
        }
        let bytes = STANDARD
            .decode(fragment.data_base64.as_bytes())
            .map_err(|_| invalid("large value fragment base64 is invalid."))?;
        if bytes.is_empty()
            || bytes.len() > self.descriptor.cursor.max_chunk_bytes
            || bytes.len() > MAX_LARGE_VALUE_CHUNK_BYTES
            || fragment.offset >= self.descriptor.byte_length
            || fragment.next_offset != fragment.offset.saturating_add(bytes.len())
            || fragment.next_offset > self.descriptor.byte_length
            || fragment.complete != (fragment.next_offset == self.descriptor.byte_length)
        {
            return Err(invalid("large value fragment byte range is invalid."));
        }
        if let Some(existing) = self.fragments.get(&fragment.offset) {
            if existing != &bytes {
                return Err(invalid(
                    "large value fragments conflict at the same offset.",
                ));
            }
            return Ok(());
        }
        self.saw_complete |= fragment.complete;
        self.fragments.insert(fragment.offset, bytes);
        Ok(())
    }

    pub fn finish(self) -> Result<Value, DevupError> {
        if !self.saw_complete {
            return Err(invalid(
                "large value fragment for the final range is missing.",
            ));
        }
        let mut output = Vec::with_capacity(self.descriptor.byte_length);
        for (offset, bytes) in self.fragments {
            if offset != output.len() {
                return Err(invalid(
                    "large value fragment ranges are missing or overlapping.",
                ));
            }
            output.extend_from_slice(&bytes);
        }
        if output.len() != self.descriptor.byte_length
            || sha256_hex(&output) != self.descriptor.sha256
        {
            return Err(invalid(
                "large value fragment length or hash does not match.",
            ));
        }
        serde_json::from_slice(&output)
            .map_err(|_| invalid("large value fragment cannot be restored as a JSON value."))
    }
}

pub(crate) fn descriptors_in_chunk(
    chunk: &SnapshotChunk,
) -> Result<Vec<LargeValueDescriptor>, DevupError> {
    let mut descriptors = Vec::new();
    for node in &chunk.nodes {
        for (field, value) in node.fields.iter().chain(&node.extra) {
            let Some(raw) = value.get("$largeValue") else {
                continue;
            };
            let descriptor: LargeValueDescriptor = serde_json::from_value(raw.clone())
                .map_err(|_| invalid("snapshot large value descriptor format is invalid."))?;
            validate_descriptor(&descriptor)?;
            if descriptor.node_id != node.id || descriptor.field != *field {
                return Err(invalid(
                    "snapshot large value descriptor target does not match its field.",
                ));
            }
            descriptors.push(descriptor);
        }
    }
    descriptors.sort_by(|left, right| {
        left.node_id
            .cmp(&right.node_id)
            .then_with(|| left.field.cmp(&right.field))
    });
    descriptors.dedup();
    Ok(descriptors)
}

pub(crate) fn large_value_from_result(
    result: &UpstreamResult,
) -> Result<LargeValueResult, DevupError> {
    find_large_value_result(&result.raw)
        .ok_or_else(|| invalid("large value fragment not found in the Figma MCP response."))
}

pub(crate) fn replace_descriptor(
    chunks: &mut BTreeMap<usize, SnapshotChunk>,
    descriptor: &LargeValueDescriptor,
    value: Value,
) -> Result<(), DevupError> {
    for chunk in chunks.values_mut() {
        if let Some(node) = chunk
            .nodes
            .iter_mut()
            .find(|node| node.id == descriptor.node_id)
        {
            let slot = node
                .fields
                .get_mut(&descriptor.field)
                .or_else(|| node.extra.get_mut(&descriptor.field))
                .ok_or_else(|| {
                    invalid("field referenced by the large value descriptor is missing.")
                })?;
            let observed: LargeValueDescriptor = serde_json::from_value(
                slot.get("$largeValue")
                    .cloned()
                    .ok_or_else(|| invalid("large value descriptor marker is missing."))?,
            )
            .map_err(|_| invalid("large value descriptor marker is invalid."))?;
            if observed != *descriptor {
                return Err(invalid("large value descriptor changed during collection."));
            }
            *slot = value;
            node.field_errors.remove(&descriptor.field);
            return Ok(());
        }
    }
    Err(invalid("node for the large value descriptor not found."))
}

fn validate_descriptor(descriptor: &LargeValueDescriptor) -> Result<(), DevupError> {
    if descriptor.node_id.is_empty()
        || descriptor.field.is_empty()
        || descriptor.byte_length == 0
        || descriptor.byte_length > MAX_LARGE_VALUE_BYTES
        || descriptor.sha256.len() != 64
        || !descriptor
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || descriptor.cursor.next_offset != 0
        || descriptor.cursor.max_chunk_bytes == 0
        || descriptor.cursor.max_chunk_bytes > MAX_LARGE_VALUE_CHUNK_BYTES
    {
        return Err(invalid("large value descriptor range or hash is invalid."));
    }
    Ok(())
}

fn find_large_value_result(value: &Value) -> Option<LargeValueResult> {
    if let Ok(fragment) = serde_json::from_value::<LargeValueFragment>(value.clone())
        && !fragment.file_key.is_empty()
        && !fragment.node_id.is_empty()
        && !fragment.field.is_empty()
    {
        return Some(LargeValueResult::Fragment(fragment));
    }
    if value.get("kind").and_then(Value::as_str) == Some("devupLargeValueUnsupported")
        && let Ok(unsupported) = serde_json::from_value::<LargeValueUnsupported>(value.clone())
    {
        return Some(LargeValueResult::Unsupported(unsupported));
    }
    match value {
        Value::Object(object) => object.values().find_map(find_large_value_result),
        Value::Array(values) => values.iter().find_map(find_large_value_result),
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|value| find_large_value_result(&value)),
        _ => None,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn invalid(message: &str) -> DevupError {
    DevupError::new(ErrorCode::DevupSnapshotUnsupported, message, false)
}
