use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{DevupError, Diagnostic, ErrorCode, Snapshot, UpstreamResult};

pub const MAX_ASSET_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetFormat {
    Png,
    Jpg,
    Svg,
    Pdf,
}

impl AssetFormat {
    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpg => "image/jpeg",
            Self::Svg => "image/svg+xml",
            Self::Pdf => "application/pdf",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpg => "jpg",
            Self::Svg => "svg",
            Self::Pdf => "pdf",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetStatus {
    Available,
    Exported,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetRequest {
    pub asset_id: String,
    pub node_id: String,
    pub field: String,
    pub image_hash: Option<String>,
    pub format: AssetFormat,
    pub scale: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetSelection {
    pub asset_id: String,
    pub format: AssetFormat,
    pub scale: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetManifestEntry {
    pub asset_id: String,
    pub node_id: String,
    pub field: String,
    pub source_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<AssetFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<u8>,
    pub status: AssetStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_length: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_base64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetManifest {
    pub version: u32,
    pub assets: Vec<AssetManifestEntry>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn discover_asset_manifest(snapshot: &Snapshot) -> AssetManifest {
    let mut assets = Vec::new();
    for node in snapshot.nodes.values() {
        if let Some(fills) = node.typed_view().value("fills").and_then(Value::as_array) {
            for (index, fill) in fills.iter().enumerate() {
                if fill.get("type").and_then(Value::as_str) != Some("IMAGE") {
                    continue;
                }
                let image_hash = fill
                    .get("imageHash")
                    .or_else(|| fill.get("imageRef"))
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                assets.push(AssetManifestEntry {
                    asset_id: format!("{}:fills:{index}", node.id),
                    node_id: node.id.clone(),
                    field: format!("fills/{index}"),
                    source_kind: "image-fill".to_owned(),
                    image_hash,
                    format: None,
                    scale: None,
                    status: AssetStatus::Available,
                    byte_length: None,
                    sha256: None,
                    mime_type: None,
                    data_base64: None,
                    output_path: None,
                    error_code: None,
                });
            }
        }
        if matches!(
            node.node_type.as_str(),
            "VECTOR" | "BOOLEAN_OPERATION" | "STAR" | "LINE" | "ELLIPSE" | "POLYGON"
        ) {
            assets.push(AssetManifestEntry {
                asset_id: format!("{}:node", node.id),
                node_id: node.id.clone(),
                field: "node".to_owned(),
                source_kind: "vector-node".to_owned(),
                image_hash: None,
                format: None,
                scale: None,
                status: AssetStatus::Available,
                byte_length: None,
                sha256: None,
                mime_type: None,
                data_base64: None,
                output_path: None,
                error_code: None,
            });
        }
    }
    assets.sort_by(|left, right| left.asset_id.cmp(&right.asset_id));
    AssetManifest {
        version: 1,
        assets,
        diagnostics: Vec::new(),
    }
}

pub fn validate_asset_requests(
    snapshot: &Snapshot,
    requests: &[AssetRequest],
) -> Result<(), DevupError> {
    if requests.len() > 16 {
        return Err(invalid("At most 16 assets can be exported at once."));
    }
    let available = discover_asset_manifest(snapshot);
    let mut seen = std::collections::BTreeSet::new();
    for request in requests {
        if request.scale == 0 || request.scale > 4 || !seen.insert(request.asset_id.as_str()) {
            return Err(invalid(
                "asset request has an invalid scale or a duplicate ID.",
            ));
        }
        let Some(candidate) = available
            .assets
            .iter()
            .find(|asset| asset.asset_id == request.asset_id)
        else {
            return Err(invalid("The requested asset is not in the snapshot."));
        };
        if candidate.node_id != request.node_id
            || candidate.field != request.field
            || candidate.image_hash != request.image_hash
        {
            return Err(invalid("asset request does not match the snapshot source."));
        }
    }
    Ok(())
}

pub fn resolve_asset_selections(
    snapshot: &Snapshot,
    selections: &[AssetSelection],
) -> Result<Vec<AssetRequest>, DevupError> {
    if selections.len() > 16 {
        return Err(invalid("At most 16 assets can be exported at once."));
    }
    let manifest = discover_asset_manifest(snapshot);
    let mut seen = std::collections::BTreeSet::new();
    let requests = selections
        .iter()
        .map(|selection| {
            if selection.scale == 0
                || selection.scale > 4
                || !seen.insert(selection.asset_id.as_str())
            {
                return Err(invalid(
                    "asset selection has an invalid scale or a duplicate ID.",
                ));
            }
            let asset = manifest
                .assets
                .iter()
                .find(|asset| asset.asset_id == selection.asset_id)
                .ok_or_else(|| invalid("The selected asset is not in the snapshot."))?;
            Ok(AssetRequest {
                asset_id: asset.asset_id.clone(),
                node_id: asset.node_id.clone(),
                field: asset.field.clone(),
                image_hash: asset.image_hash.clone(),
                format: selection.format,
                scale: selection.scale,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    validate_asset_requests(snapshot, &requests)?;
    Ok(requests)
}

pub fn asset_export_from_result(
    result: &UpstreamResult,
    file_key: &str,
    version: Option<&str>,
    request: &AssetRequest,
) -> Result<AssetManifestEntry, DevupError> {
    let descriptor = find_descriptor(&result.raw)
        .ok_or_else(|| invalid("asset descriptor not found in the Figma MCP response."))?;
    if descriptor.file_key != file_key
        || descriptor.version.as_deref() != version
        || descriptor.asset_id != request.asset_id
        || descriptor.node_id != request.node_id
        || descriptor.field != request.field
        || descriptor.image_hash != request.image_hash
        || descriptor.format != request.format
        || descriptor.scale != request.scale
    {
        return Err(invalid(
            "asset descriptor target or version does not match the request.",
        ));
    }
    let source_kind = if request.image_hash.is_some() {
        "image-fill"
    } else {
        "vector-node"
    };
    if descriptor.status == AssetStatus::Failed {
        return Ok(AssetManifestEntry {
            asset_id: request.asset_id.clone(),
            node_id: request.node_id.clone(),
            field: request.field.clone(),
            source_kind: source_kind.to_owned(),
            image_hash: request.image_hash.clone(),
            format: Some(request.format),
            scale: Some(request.scale),
            status: AssetStatus::Failed,
            byte_length: None,
            sha256: None,
            mime_type: None,
            data_base64: None,
            output_path: None,
            error_code: descriptor.error_code,
        });
    }
    let payload = find_payload(&result.raw, request.format.mime_type()).ok_or_else(|| {
        // Which shapes the response *did* carry. Without this the failure is
        // indistinguishable between "no attachment came back", "it came back
        // under a different mime type" and "it came back in a field this
        // search does not read" — three very different bugs.
        DevupError::with_details(
            ErrorCode::DevupSnapshotUnsupported,
            "asset export response does not contain the requested binary.",
            false,
            json!({
                "expectedMimeType": request.format.mime_type(),
                "observed": observed_payload_shapes(&result.raw),
            }),
        )
    })?;
    let (bytes, data) = match payload {
        AssetPayload::Base64(data) => {
            let bytes = STANDARD
                .decode(data.as_bytes())
                .map_err(|_| invalid("asset export binary base64 is invalid."))?;
            (bytes, data)
        }
        // Re-encoded so every consumer downstream still receives base64,
        // regardless of how the upstream happened to carry the payload.
        AssetPayload::Text(text) => {
            let bytes = text.into_bytes();
            let data = STANDARD.encode(&bytes);
            (bytes, data)
        }
    };
    if bytes.is_empty()
        || bytes.len() > MAX_ASSET_BYTES
        || descriptor.byte_length != Some(bytes.len())
        || descriptor.sha256.as_deref() != Some(sha256_hex(&bytes).as_str())
    {
        return Err(invalid(
            "asset export binary length or hash does not match.",
        ));
    }
    Ok(AssetManifestEntry {
        asset_id: request.asset_id.clone(),
        node_id: request.node_id.clone(),
        field: request.field.clone(),
        source_kind: source_kind.to_owned(),
        image_hash: request.image_hash.clone(),
        format: Some(request.format),
        scale: Some(request.scale),
        status: AssetStatus::Exported,
        byte_length: Some(bytes.len()),
        sha256: descriptor.sha256,
        mime_type: Some(request.format.mime_type().to_owned()),
        data_base64: Some(data),
        output_path: None,
        error_code: None,
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetExportDescriptor {
    kind: String,
    file_key: String,
    version: Option<String>,
    asset_id: String,
    node_id: String,
    field: String,
    image_hash: Option<String>,
    format: AssetFormat,
    scale: u8,
    status: AssetStatus,
    byte_length: Option<usize>,
    sha256: Option<String>,
    error_code: Option<String>,
}

fn find_descriptor(value: &Value) -> Option<AssetExportDescriptor> {
    if let Ok(descriptor) = serde_json::from_value::<AssetExportDescriptor>(value.clone())
        && descriptor.kind == "devupAssetExport"
    {
        return Some(descriptor);
    }
    match value {
        Value::Object(object) => object.values().find_map(find_descriptor),
        Value::Array(values) => values.iter().find_map(find_descriptor),
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|value| find_descriptor(&value)),
        _ => None,
    }
}

/// How an upstream carried the exported asset.
enum AssetPayload {
    /// An image content block or a blob resource, which are base64.
    Base64(String),
    /// A text resource. MCP models a text-based document — SVG being the one
    /// devup-mcp exports — as `text` holding the document itself rather than
    /// base64 of it, so an SVG export used to be invisible to a search that
    /// only looked for `data`/`blob` and every request failed with "asset
    /// export response does not contain the requested binary".
    Text(String),
}

fn find_payload(value: &Value, mime_type: &str) -> Option<AssetPayload> {
    match value {
        Value::Object(object) => {
            if object.get("mimeType").and_then(Value::as_str) == Some(mime_type) {
                // Base64 first: when a payload offers both, the binary form is
                // the exact bytes, while `text` may be a lossy preview.
                if let Some(data) = object
                    .get("data")
                    .or_else(|| object.get("blob"))
                    .and_then(Value::as_str)
                {
                    return Some(AssetPayload::Base64(data.to_owned()));
                }
                if let Some(text) = object.get("text").and_then(Value::as_str) {
                    return Some(AssetPayload::Text(text.to_owned()));
                }
            }
            object
                .values()
                .find_map(|value| find_payload(value, mime_type))
        }
        Value::Array(values) => values
            .iter()
            .find_map(|value| find_payload(value, mime_type)),
        // The descriptor — and, for SVG, the payload inlined beside it —
        // arrives as JSON inside a text content block, so the search has to
        // step through that encoding exactly as `find_descriptor` does.
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|value| find_payload(&value, mime_type)),
        _ => None,
    }
}

/// Describes every payload-carrying object in a response by its `type` and
/// `mimeType` and which of `data`/`blob`/`text` it holds, without ever
/// including the payload itself. Bounded so a large response cannot turn a
/// diagnostic into another problem.
fn observed_payload_shapes(value: &Value) -> Vec<String> {
    fn walk(value: &Value, found: &mut Vec<String>) {
        if found.len() >= 12 {
            return;
        }
        match value {
            Value::Object(object) => {
                let carriers: Vec<&str> = ["data", "blob", "text", "uri"]
                    .into_iter()
                    .filter(|key| object.contains_key(*key))
                    .collect();
                if !carriers.is_empty() {
                    let kind = object
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("<no type>");
                    let mime = object
                        .get("mimeType")
                        .and_then(Value::as_str)
                        .unwrap_or("<no mimeType>");
                    found.push(format!(
                        "type={kind} mimeType={mime} carries=[{}]",
                        carriers.join(",")
                    ));
                }
                for child in object.values() {
                    walk(child, found);
                }
            }
            Value::Array(values) => {
                for child in values {
                    walk(child, found);
                }
            }
            _ => {}
        }
    }

    let mut found = Vec::new();
    walk(value, &mut found);
    found
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
