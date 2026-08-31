use std::collections::BTreeSet;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    DevupError, ErrorCode, FigmaTarget, ResourceKind, SnapshotChunk, UpstreamResult,
    collect_used_resource_refs,
};

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const ENVELOPE_CHUNK_TYPE: &[u8; 4] = b"duVp";
const EXPECTED_IHDR: &[u8; 13] = &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0];
const MAX_PNG_BYTES: usize = 11 * 1024 * 1024;
const MAX_BASE64_PNG_BYTES: usize = MAX_PNG_BYTES.div_ceil(3) * 4;
const MAX_ENVELOPE_BYTES: usize = 8 * 1024 * 1024;

type EnvelopeChunk<'a> = (u32, u32, &'a [u8]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FastTransportStats {
    pub raw_bytes: usize,
    pub wire_bytes: usize,
    pub chunk_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FastSnapshotPayload {
    pub snapshot: SnapshotChunk,
    pub resources: UpstreamResult,
    pub stats: FastTransportStats,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Envelope {
    schema_version: u32,
    source: EnvelopeSource,
    snapshot: SnapshotChunk,
    resources: Value,
    integrity: EnvelopeIntegrity,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvelopeSource {
    file_key: String,
    root_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvelopeIntegrity {
    node_count: usize,
    variable_ref_count: usize,
    style_ref_count: usize,
    utf8_bytes: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvelopeDescriptor {
    kind: String,
    schema_version: u32,
    root_id: String,
    node_count: usize,
    variable_ref_count: usize,
    style_ref_count: usize,
    utf8_bytes: usize,
    chunk_count: usize,
}

pub fn decode_fast_snapshot(
    result: &UpstreamResult,
    target: &FigmaTarget,
) -> Result<FastSnapshotPayload, DevupError> {
    let descriptor = find_descriptor(&result.raw)?;
    let (encoded, mime_type) = find_image(&result.raw)?;
    if mime_type != "image/png" {
        return Err(invalid("imageMime"));
    }
    if encoded.len() > MAX_BASE64_PNG_BYTES {
        return Err(too_large("png"));
    }
    let png = STANDARD
        .decode(encoded)
        .map_err(|_| invalid("imageBase64"))?;
    if png.len() > MAX_PNG_BYTES {
        return Err(too_large("png"));
    }

    let chunks = decode_png_envelope(&png)?;
    if chunks.len() != descriptor.chunk_count {
        return Err(invalid("descriptorChunkCount"));
    }
    let envelope_bytes = join_envelope_chunks(chunks)?;
    if envelope_bytes.len() > MAX_ENVELOPE_BYTES {
        return Err(too_large("envelope"));
    }
    let envelope_text =
        std::str::from_utf8(&envelope_bytes).map_err(|_| invalid("envelopeUtf8"))?;
    let envelope: Envelope =
        serde_json::from_str(envelope_text).map_err(|_| invalid("envelopeJson"))?;
    validate_envelope(&envelope, &descriptor, target, envelope_bytes.len())?;

    Ok(FastSnapshotPayload {
        snapshot: envelope.snapshot,
        resources: UpstreamResult {
            raw: envelope.resources,
        },
        stats: FastTransportStats {
            raw_bytes: envelope_bytes.len(),
            wire_bytes: png.len(),
            chunk_count: descriptor.chunk_count,
        },
    })
}

fn find_image(value: &Value) -> Result<(&str, &str), DevupError> {
    fn collect<'a>(value: &'a Value, found: &mut Vec<(&'a str, &'a str)>) {
        match value {
            Value::Object(object) => {
                if object.get("type").and_then(Value::as_str) == Some("image")
                    && let Some(data) = object.get("data").and_then(Value::as_str)
                    && let Some(mime) = object
                        .get("mimeType")
                        .or_else(|| object.get("mime_type"))
                        .and_then(Value::as_str)
                {
                    found.push((data, mime));
                }
                for child in object.values() {
                    collect(child, found);
                }
            }
            Value::Array(values) => {
                for child in values {
                    collect(child, found);
                }
            }
            _ => {}
        }
    }

    let mut images = Vec::new();
    collect(value, &mut images);
    match images.as_slice() {
        [image] => Ok(*image),
        [] => Err(invalid("imageMissing")),
        _ => Err(invalid("imageMultiplicity")),
    }
}

fn find_descriptor(value: &Value) -> Result<EnvelopeDescriptor, DevupError> {
    fn collect(value: &Value, found: &mut Vec<EnvelopeDescriptor>) {
        match value {
            Value::Object(object) => {
                if let Some(Value::String(text)) = object.get("text")
                    && let Ok(descriptor) = serde_json::from_str::<EnvelopeDescriptor>(text)
                    && descriptor.kind == "devupFastSnapshotDescriptor"
                {
                    found.push(descriptor);
                }
                for child in object.values() {
                    collect(child, found);
                }
            }
            Value::Array(values) => {
                for child in values {
                    collect(child, found);
                }
            }
            _ => {}
        }
    }

    let mut descriptors = Vec::new();
    collect(value, &mut descriptors);
    match descriptors.len() {
        1 => Ok(descriptors.remove(0)),
        0 => Err(invalid("descriptorMissing")),
        _ => Err(invalid("descriptorMultiplicity")),
    }
}

fn decode_png_envelope(png: &[u8]) -> Result<Vec<EnvelopeChunk<'_>>, DevupError> {
    if !png.starts_with(PNG_SIGNATURE) {
        return Err(invalid("pngSignature"));
    }

    let mut offset = PNG_SIGNATURE.len();
    let mut first = true;
    let mut saw_idat = false;
    let mut saw_iend = false;
    let mut envelope_chunks = Vec::new();
    while offset < png.len() {
        let header_end = offset.checked_add(8).ok_or_else(|| invalid("pngLength"))?;
        if header_end > png.len() {
            return Err(invalid("pngLength"));
        }
        let length = u32::from_be_bytes(
            png[offset..offset + 4]
                .try_into()
                .map_err(|_| invalid("pngLength"))?,
        ) as usize;
        let chunk_type: &[u8; 4] = png[offset + 4..header_end]
            .try_into()
            .map_err(|_| invalid("pngChunkType"))?;
        let data_start = header_end;
        let data_end = data_start
            .checked_add(length)
            .ok_or_else(|| invalid("pngLength"))?;
        let crc_end = data_end
            .checked_add(4)
            .ok_or_else(|| invalid("pngLength"))?;
        if crc_end > png.len() {
            return Err(invalid("pngLength"));
        }

        if first {
            if chunk_type != b"IHDR" || &png[data_start..data_end] != EXPECTED_IHDR {
                return Err(invalid("pngIhdr"));
            }
        } else if chunk_type == b"IHDR" {
            return Err(invalid("pngIhdr"));
        }
        first = false;
        let expected_crc = u32::from_be_bytes(
            png[data_end..crc_end]
                .try_into()
                .map_err(|_| invalid("pngCrc"))?,
        );
        if crc32(&png[offset + 4..data_end]) != expected_crc {
            return Err(invalid("pngCrc"));
        }
        if chunk_type == ENVELOPE_CHUNK_TYPE {
            if length < 8 {
                return Err(invalid("envelopeChunkHeader"));
            }
            let sequence = u32::from_be_bytes(
                png[data_start..data_start + 4]
                    .try_into()
                    .map_err(|_| invalid("envelopeChunkHeader"))?,
            );
            let total = u32::from_be_bytes(
                png[data_start + 4..data_start + 8]
                    .try_into()
                    .map_err(|_| invalid("envelopeChunkHeader"))?,
            );
            envelope_chunks.push((sequence, total, &png[data_start + 8..data_end]));
        }
        if chunk_type == b"IDAT" {
            saw_idat = true;
        }
        if chunk_type == b"IEND" {
            if length != 0 || crc_end != png.len() {
                return Err(invalid("pngIend"));
            }
            saw_iend = true;
            break;
        }
        offset = crc_end;
    }

    if !saw_iend {
        return Err(invalid("pngIend"));
    }
    if !saw_idat {
        return Err(invalid("pngIdat"));
    }
    if envelope_chunks.is_empty() {
        return Err(invalid("envelopeChunkMissing"));
    }
    Ok(envelope_chunks)
}

fn join_envelope_chunks(chunks: Vec<EnvelopeChunk<'_>>) -> Result<Vec<u8>, DevupError> {
    let total = u32::try_from(chunks.len()).map_err(|_| too_large("chunkCount"))?;
    let mut byte_count = 0_usize;
    for (expected_sequence, (sequence, declared_total, bytes)) in chunks.iter().enumerate() {
        if declared_total != &total || sequence != &(expected_sequence as u32) {
            return Err(invalid("envelopeChunkSequence"));
        }
        byte_count = byte_count
            .checked_add(bytes.len())
            .ok_or_else(|| too_large("envelope"))?;
        if byte_count > MAX_ENVELOPE_BYTES {
            return Err(too_large("envelope"));
        }
    }
    let mut output = Vec::with_capacity(byte_count);
    for (_, _, bytes) in chunks {
        output.extend_from_slice(bytes);
    }
    Ok(output)
}

fn validate_envelope(
    envelope: &Envelope,
    descriptor: &EnvelopeDescriptor,
    target: &FigmaTarget,
    utf8_bytes: usize,
) -> Result<(), DevupError> {
    if envelope.schema_version != 1 || descriptor.schema_version != 1 {
        return Err(invalid("schemaVersion"));
    }
    let target_root = target
        .node_id
        .as_deref()
        .ok_or_else(|| invalid("targetRootMissing"))?;
    if envelope.source.file_key != target.file_key
        || envelope.snapshot.file_key != target.file_key
        || envelope.source.root_id != target_root
        || descriptor.root_id != target_root
        || envelope.snapshot.root_ids.as_slice() != [target_root]
    {
        return Err(invalid("targetMismatch"));
    }
    if envelope.integrity.utf8_bytes != utf8_bytes || descriptor.utf8_bytes != utf8_bytes {
        return Err(invalid("utf8Bytes"));
    }

    let mut node_ids = BTreeSet::new();
    for node in &envelope.snapshot.nodes {
        if !node_ids.insert(node.id.as_str()) {
            return Err(invalid("duplicateNode"));
        }
    }
    if envelope.integrity.node_count != node_ids.len()
        || descriptor.node_count != node_ids.len()
        || !node_ids.contains(target_root)
    {
        return Err(invalid("nodeCount"));
    }
    for node in &envelope.snapshot.nodes {
        for child_id in node.typed_view().child_ids() {
            if !node_ids.contains(child_id) {
                return Err(invalid("danglingChild"));
            }
        }
    }

    let refs = collect_used_resource_refs(std::slice::from_ref(&envelope.snapshot));
    if envelope.integrity.variable_ref_count != refs.variable_ids.len()
        || descriptor.variable_ref_count != refs.variable_ids.len()
        || envelope.integrity.style_ref_count != refs.styles.len()
        || descriptor.style_ref_count != refs.styles.len()
    {
        return Err(invalid("resourceRefCount"));
    }
    validate_resources(&envelope.resources, &refs.variable_ids, &refs.styles)?;
    Ok(())
}

fn validate_resources(
    resources: &Value,
    variable_ids: &[String],
    style_refs: &[crate::ResourceStyleRef],
) -> Result<(), DevupError> {
    let object = resources
        .as_object()
        .ok_or_else(|| invalid("resourcesShape"))?;
    let resolved_variables = resource_ids(object.get("variables"), "variables")?;
    let resolved_styles = resource_ids(object.get("styles"), "styles")?;
    let mut unresolved_variables = BTreeSet::new();
    let mut unresolved_styles = BTreeSet::new();
    for value in object
        .get("unresolved")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("unresolvedShape"))?
    {
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("unresolvedShape"))?;
        match serde_json::from_value::<ResourceKind>(
            value
                .get("kind")
                .cloned()
                .ok_or_else(|| invalid("unresolvedShape"))?,
        )
        .map_err(|_| invalid("unresolvedShape"))?
        {
            ResourceKind::Variable => {
                unresolved_variables.insert(id);
            }
            ResourceKind::Style => {
                unresolved_styles.insert(id);
            }
        }
    }
    if variable_ids.iter().any(|id| {
        !resolved_variables.contains(id.as_str()) && !unresolved_variables.contains(id.as_str())
    }) || style_refs.iter().any(|style| {
        !resolved_styles.contains(style.id.as_str())
            && !unresolved_styles.contains(style.id.as_str())
    }) {
        return Err(invalid("resourceMissing"));
    }
    Ok(())
}

fn resource_ids<'a>(
    value: Option<&'a Value>,
    category: &'static str,
) -> Result<BTreeSet<&'a str>, DevupError> {
    let values = value
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(category))?;
    values
        .iter()
        .map(|value| {
            value
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid(category))
        })
        .collect()
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn invalid(category: &'static str) -> DevupError {
    DevupError::with_details(
        ErrorCode::DevupSnapshotUnsupported,
        "Figma fast snapshot envelope 검증에 실패했습니다.",
        false,
        json!({"category": category}),
    )
}

fn too_large(category: &'static str) -> DevupError {
    DevupError::with_details(
        ErrorCode::DevupFigmaResponseTooLarge,
        "Figma fast snapshot envelope가 안전한 크기 제한을 초과했습니다.",
        false,
        json!({"category": category}),
    )
}
