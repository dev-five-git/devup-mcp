use std::str::FromStr;

use devup_mcp_figma::{DevupError, ErrorCode};
use serde::{Deserialize, Serialize};

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
}

impl ProjectedOutput {
    pub fn text(name: impl Into<String>, mime_type: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            mime_type: mime_type.into(),
            bytes,
            is_binary: false,
        }
    }

    pub fn binary(name: impl Into<String>, mime_type: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            mime_type: mime_type.into(),
            bytes,
            is_binary: true,
        }
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
    let every_output_inline = outputs
        .iter()
        .all(|output| output.bytes.len() <= MAX_INLINE_OUTPUT_BYTES);
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
