//! Original uploads, carried only by the local bridge. These bytes have no
//! paint transforms, filters, opacity, clipping or child composition applied.
//! They must never be decoded as an AssetManifestEntry's node rendition.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{AssetFormat, AssetRequest, DevupError, ErrorCode, MAX_ASSET_BYTES, UpstreamResult};

const FIELD: &str = "$original-image/fills/";

impl AssetRequest {
    /// A distinct bridge-only read of the upload used by this exact paint.
    /// Format/scale are placeholders required by the shared request envelope;
    /// the original response explicitly has neither a requested codec nor scale.
    /// Use `original_image_from_result`, never `asset_export_from_result`.
    pub fn original_image(node_id: &str, fill_index: usize, image_hash: &str) -> Self {
        Self {
            asset_id: format!("{node_id}:original-image-v1:{fill_index}:{image_hash}"),
            node_id: node_id.to_owned(),
            field: format!("{FIELD}{fill_index}"),
            image_hash: Some(image_hash.to_owned()),
            format: AssetFormat::Png,
            scale: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginalImage {
    pub bytes: Vec<u8>,
    pub mime_type: String,
    /// Intrinsic dimensions reported by Image.getSizeAsync, not a node box.
    pub width: u32,
    pub height: u32,
    pub sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Descriptor {
    representation: String,
    file_key: String,
    version: Option<String>,
    asset_id: String,
    node_id: String,
    field: String,
    image_hash: String,
    status: String,
    mime_type: String,
    width: u32,
    height: u32,
    byte_length: usize,
    sha256: String,
    data: String,
}

fn find(value: &Value) -> Option<Value> {
    if matches!(
        value["kind"].as_str(),
        Some("devupOriginalImage" | "devupAssetExport")
    ) {
        return Some(value.clone());
    }
    match value {
        Value::Object(object) => object.values().find_map(find),
        Value::Array(values) => values.iter().find_map(find),
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|v| find(&v)),
        _ => None,
    }
}

fn invalid(message: &str) -> DevupError {
    DevupError::new(ErrorCode::DevupSnapshotUnsupported, message, false)
}

/// Validate identity, byte count, SHA-256 and codec before returning original
/// bytes. A remote refusal and an old plugin's unsupported-field response stay
/// named diagnostics, rather than being mistaken for successful PNG exports.
pub fn original_image_from_result(
    result: &UpstreamResult,
    file_key: &str,
    version: Option<&str>,
    request: &AssetRequest,
) -> Result<OriginalImage, DevupError> {
    if request
        .field
        .strip_prefix(FIELD)
        .and_then(|s| s.parse::<usize>().ok())
        .is_none()
    {
        return Err(invalid(
            "original image read requires an explicit original-image field",
        ));
    }
    let value = find(&result.raw).ok_or_else(|| invalid("original image descriptor missing"))?;
    if value["status"] == "failed" {
        let code = value["errorCode"]
            .as_str()
            .unwrap_or("DEVUP_ORIGINAL_IMAGE_READ_FAILED");
        return Err(DevupError::with_details(
            ErrorCode::DevupSnapshotUnsupported,
            format!(
                "{code}: original image bytes were not delivered; the bridge requires the updated plugin bundle"
            ),
            false,
            json!({"errorCode":code}),
        ));
    }
    if value["kind"] != "devupOriginalImage"
        || !value["format"].is_null()
        || !value["scale"].is_null()
    {
        return Err(invalid(
            "a node rendition cannot substitute for original image bytes",
        ));
    }
    let descriptor: Descriptor =
        serde_json::from_value(value).map_err(|_| invalid("invalid original image descriptor"))?;
    if descriptor.representation != "original-image-v1"
        || descriptor.status != "exported"
        || descriptor.file_key != file_key
        || descriptor.version.as_deref() != version
        || descriptor.node_id != request.node_id
        || descriptor.asset_id != request.asset_id
        || descriptor.field != request.field
        || Some(&descriptor.image_hash) != request.image_hash.as_ref()
        || descriptor.width == 0
        || descriptor.height == 0
        || descriptor.byte_length == 0
        || descriptor.byte_length > MAX_ASSET_BYTES
        || descriptor.data.len() > MAX_ASSET_BYTES.div_ceil(3) * 4
    {
        return Err(invalid(
            "original image identity, dimensions or byte count does not match",
        ));
    }
    let bytes = STANDARD
        .decode(descriptor.data)
        .map_err(|_| invalid("invalid original image base64"))?;
    let sha256: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[255, 216, 255]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else {
        return Err(invalid("unsupported original image codec"));
    };
    if bytes.len() != descriptor.byte_length
        || sha256 != descriptor.sha256
        || mime != descriptor.mime_type
    {
        return Err(invalid(
            "original image bytes fail length, SHA-256 or MIME validation",
        ));
    }
    Ok(OriginalImage {
        bytes,
        mime_type: descriptor.mime_type,
        width: descriptor.width,
        height: descriptor.height,
        sha256,
    })
}
