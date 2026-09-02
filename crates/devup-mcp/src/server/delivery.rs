use std::str::FromStr;

use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use devup_mcp_figma::{DevupError, ErrorCode};
use rand::Rng;
use rmcp::model::{CallToolResult, ContentBlock, MetaObject, Resource};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_INLINE_OUTPUT_BYTES: usize = 256 * 1024;
pub const MAX_INLINE_TOTAL_BYTES: usize = 1024 * 1024;
pub const RESOURCE_CHUNK_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum DeliveryMode {
    #[default]
    Auto,
    Inline,
    Resource,
}

impl FromStr for DeliveryMode {
    type Err = DevupError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto" => Ok(Self::Auto),
            "inline" => Ok(Self::Inline),
            "resource" => Ok(Self::Resource),
            _ => Err(DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "delivery는 auto, inline 또는 resource여야 합니다.",
                false,
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedOutput {
    pub name: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
    pub is_binary: bool,
    pub(crate) resource_id: String,
    pub(crate) asset_id: Option<String>,
}

impl ProjectedOutput {
    pub fn text(name: impl Into<String>, mime_type: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            mime_type: mime_type.into(),
            bytes,
            is_binary: false,
            resource_id: random_resource_id(),
            asset_id: None,
        }
    }

    pub fn binary(name: impl Into<String>, mime_type: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            mime_type: mime_type.into(),
            bytes,
            is_binary: true,
            resource_id: random_resource_id(),
            asset_id: None,
        }
    }

    pub(crate) fn asset(
        name: impl Into<String>,
        mime_type: impl Into<String>,
        bytes: Vec<u8>,
        asset_id: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            mime_type: mime_type.into(),
            bytes,
            is_binary: true,
            resource_id: random_resource_id(),
            asset_id: Some(asset_id.into()),
        }
    }

    pub(crate) fn manifest_uri(&self, artifact_id: &str) -> String {
        format!(
            "devup://artifact/{artifact_id}/outputs/{}/manifest",
            self.resource_id
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeliveryDecision {
    pub inline: bool,
}

pub fn choose_delivery(
    mode: DeliveryMode,
    outputs: &[ProjectedOutput],
) -> Result<DeliveryDecision, DevupError> {
    let total_bytes = outputs.iter().try_fold(0_usize, |total, output| {
        total.checked_add(output.bytes.len()).ok_or_else(|| {
            DevupError::new(
                ErrorCode::DevupFigmaResponseTooLarge,
                "생성 output 크기가 안전한 범위를 초과했습니다.",
                false,
            )
        })
    })?;
    let every_output_inline = outputs.iter().try_fold(true, |within_limit, output| {
        let wire_bytes = projected_output_wire_bytes(output)?;
        Ok::<_, DevupError>(within_limit && wire_bytes <= MAX_INLINE_OUTPUT_BYTES)
    })?;
    match mode {
        DeliveryMode::Auto => Ok(DeliveryDecision {
            inline: every_output_inline && total_bytes <= MAX_INLINE_TOTAL_BYTES,
        }),
        DeliveryMode::Inline if total_bytes > MAX_INLINE_TOTAL_BYTES => Err(DevupError::new(
            ErrorCode::DevupFigmaResponseTooLarge,
            "inline output이 1 MiB 상한을 초과했습니다. delivery=auto 또는 resource를 사용하세요.",
            false,
        )),
        DeliveryMode::Inline => Ok(DeliveryDecision { inline: true }),
        DeliveryMode::Resource => Ok(DeliveryDecision { inline: false }),
    }
}

fn projected_output_wire_bytes(output: &ProjectedOutput) -> Result<usize, DevupError> {
    let value = if output.is_binary {
        serde_json::json!({
            "output": {
                "mimeType": output.mime_type,
                "dataBase64": STANDARD.encode(&output.bytes)
            }
        })
    } else {
        let text = std::str::from_utf8(&output.bytes).map_err(|_| {
            DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "inline text output은 UTF-8이어야 합니다.",
                false,
            )
        })?;
        serde_json::json!({"output": text})
    };
    serde_json::to_vec(&CallToolResult::structured(value))
        .map(|bytes| bytes.len())
        .map_err(|error| {
            DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                format!("MCP output 크기를 계산할 수 없습니다: {error}"),
                false,
            )
        })
}

pub fn choose_delivery_for_result(
    mode: DeliveryMode,
    result: &Value,
    outputs: &[ProjectedOutput],
) -> Result<DeliveryDecision, DevupError> {
    let projected = choose_delivery(mode, outputs)?;
    let wire_bytes = serde_json::to_vec(&CallToolResult::structured(result.clone()))
        .map_err(|error| {
            DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                format!("MCP tool response 크기를 계산할 수 없습니다: {error}"),
                false,
            )
        })?
        .len();
    match mode {
        DeliveryMode::Auto => Ok(DeliveryDecision {
            inline: projected.inline && wire_bytes <= MAX_INLINE_TOTAL_BYTES,
        }),
        DeliveryMode::Inline if wire_bytes > MAX_INLINE_TOTAL_BYTES => Err(DevupError::new(
            ErrorCode::DevupFigmaResponseTooLarge,
            "직렬화된 inline MCP response가 1 MiB 상한을 초과했습니다. delivery=auto 또는 resource를 사용하세요.",
            false,
        )),
        DeliveryMode::Inline => Ok(DeliveryDecision { inline: true }),
        DeliveryMode::Resource => Ok(DeliveryDecision { inline: false }),
    }
}

pub fn tool_result(value: Value) -> CallToolResult {
    let links = value
        .get("resources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|summary| {
            let uri = summary.get("uri")?.as_str()?;
            let name = summary.get("name")?.as_str()?;
            let mime_type = summary.get("mimeType")?.as_str()?;
            let size = summary.get("size")?.as_u64()?;
            let mut meta = MetaObject::new();
            if let Some(hash) = summary.get("contentHash").and_then(Value::as_str) {
                meta.0.insert("payloadSha256".to_owned(), Value::from(hash));
            }
            meta.0
                .insert("payloadMimeType".to_owned(), Value::from(mime_type));
            meta.0.insert("payloadBytes".to_owned(), Value::from(size));
            if let Some(expires_at) = summary.get("expiresAt").and_then(Value::as_str) {
                meta.0
                    .insert("expiresAt".to_owned(), Value::from(expires_at));
            }
            Some(ContentBlock::resource_link(
                Resource::new(uri, name)
                    .with_title(format!("Devup {name} output"))
                    .with_description(
                        "Generated Devup output; read the linked manifest for bounded chunks",
                    )
                    .with_mime_type("application/json")
                    .with_meta(meta),
            ))
        })
        .collect::<Vec<_>>();
    let mut result = CallToolResult::structured(value);
    result.content.extend(links);
    result
}

fn random_resource_id() -> String {
    let mut bytes = [0_u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}
