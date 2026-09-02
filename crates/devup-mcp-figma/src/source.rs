use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{DevupError, ErrorCode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourcePolicy {
    #[default]
    Auto,
    Direct,
    Host,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectedSource {
    Direct,
    Host,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamFailureContext {
    RegisterClient,
    Connect,
    ListTools,
    CallTool,
    Decode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamFailureKind {
    CatalogRejected,
    AuthUnavailable,
    CapabilityUnavailable,
    PermissionDenied,
    RateLimited,
    NodeNotFound,
    VersionChanged,
    Transport,
    InvalidResponse,
}

pub fn fallback_allowed(policy: SourcePolicy, kind: UpstreamFailureKind) -> bool {
    policy == SourcePolicy::Auto
        && matches!(
            kind,
            UpstreamFailureKind::CatalogRejected
                | UpstreamFailureKind::AuthUnavailable
                | UpstreamFailureKind::CapabilityUnavailable
                | UpstreamFailureKind::PermissionDenied
        )
}

pub fn fallback_allowed_for_error(policy: SourcePolicy, error: &DevupError) -> bool {
    let kind = match error.code {
        ErrorCode::DevupFigmaCatalogRejected => UpstreamFailureKind::CatalogRejected,
        ErrorCode::DevupAuthRequired => UpstreamFailureKind::AuthUnavailable,
        ErrorCode::DevupFigmaDirectUnavailable => UpstreamFailureKind::CapabilityUnavailable,
        ErrorCode::DevupFigmaPermissionDenied => UpstreamFailureKind::PermissionDenied,
        ErrorCode::DevupFigmaRateLimited => UpstreamFailureKind::RateLimited,
        ErrorCode::DevupFigmaNodeNotFound => UpstreamFailureKind::NodeNotFound,
        ErrorCode::DevupFigmaVersionChanged => UpstreamFailureKind::VersionChanged,
        _ => return false,
    };
    fallback_allowed(policy, kind)
}

pub fn classify_upstream_failure(
    context: UpstreamFailureContext,
    status: Option<u16>,
    message: &str,
) -> UpstreamFailureKind {
    let message = message.to_ascii_lowercase();
    if message.contains("catalog")
        || message.contains("not approved")
        || (context == UpstreamFailureContext::RegisterClient && status == Some(403))
    {
        return UpstreamFailureKind::CatalogRejected;
    }
    if status == Some(401) || message.contains("unauthorized") || message.contains("status 401") {
        return UpstreamFailureKind::AuthUnavailable;
    }
    if context == UpstreamFailureContext::ListTools
        && (message.contains("unavailable")
            || message.contains("missing")
            || message.contains("required tool"))
    {
        return UpstreamFailureKind::CapabilityUnavailable;
    }
    if status == Some(403) {
        return UpstreamFailureKind::PermissionDenied;
    }
    if status == Some(429) || message.contains("status 429") || message.contains("rate limit") {
        return UpstreamFailureKind::RateLimited;
    }
    if (status == Some(404) && message.contains("node")) || message.contains("node not found") {
        return UpstreamFailureKind::NodeNotFound;
    }
    if message.contains("version changed") || message.contains("version conflict") {
        return UpstreamFailureKind::VersionChanged;
    }
    if context == UpstreamFailureContext::Decode {
        return UpstreamFailureKind::InvalidResponse;
    }
    UpstreamFailureKind::Transport
}

pub fn upstream_failure_error(
    context: UpstreamFailureContext,
    status: Option<u16>,
    message: &str,
) -> DevupError {
    classify_upstream_failure(context, status, message).into_devup_error(status)
}

impl UpstreamFailureKind {
    pub fn into_devup_error(self, status: Option<u16>) -> DevupError {
        let (code, message, retryable) = match self {
            Self::CatalogRejected => (
                ErrorCode::DevupFigmaCatalogRejected,
                "이 client는 Figma MCP Catalog에서 승인되지 않았습니다.",
                false,
            ),
            Self::AuthUnavailable => (
                ErrorCode::DevupAuthRequired,
                "Figma direct 연결 인증을 사용할 수 없습니다.",
                false,
            ),
            Self::CapabilityUnavailable => (
                ErrorCode::DevupFigmaDirectUnavailable,
                "Figma direct 연결에 필요한 읽기 capability가 없습니다.",
                false,
            ),
            Self::PermissionDenied => (
                ErrorCode::DevupFigmaPermissionDenied,
                "Figma 파일을 읽을 권한이 없습니다.",
                false,
            ),
            Self::RateLimited => (
                ErrorCode::DevupFigmaRateLimited,
                "Figma 요청 한도에 도달했습니다.",
                true,
            ),
            Self::NodeNotFound => (
                ErrorCode::DevupFigmaNodeNotFound,
                "Figma node를 찾지 못했습니다.",
                false,
            ),
            Self::VersionChanged => (
                ErrorCode::DevupFigmaVersionChanged,
                "수집 중 Figma 파일 버전이 변경되었습니다.",
                true,
            ),
            Self::Transport => (
                ErrorCode::DevupFigmaDirectUnavailable,
                "Figma direct 연결을 완료하지 못했습니다.",
                true,
            ),
            Self::InvalidResponse => (
                ErrorCode::DevupSnapshotUnsupported,
                "Figma MCP 응답을 안전하게 해석하지 못했습니다.",
                false,
            ),
        };
        DevupError::with_details(
            code,
            message,
            retryable,
            json!({ "source": "direct", "status": status }),
        )
    }
}
