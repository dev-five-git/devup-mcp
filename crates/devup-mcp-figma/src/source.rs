use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{DevupError, ErrorCode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourcePolicy {
    #[default]
    Auto,
    Direct,
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
                "This client is not approved in the Figma MCP Catalog.",
                false,
            ),
            Self::AuthUnavailable => (
                ErrorCode::DevupAuthRequired,
                "Figma direct connection authentication is unavailable.",
                false,
            ),
            Self::CapabilityUnavailable => (
                ErrorCode::DevupFigmaDirectUnavailable,
                "The read capability required for a Figma direct connection is missing.",
                false,
            ),
            Self::PermissionDenied => (
                ErrorCode::DevupFigmaPermissionDenied,
                "No permission to read this Figma file.",
                false,
            ),
            Self::RateLimited => (
                ErrorCode::DevupFigmaRateLimited,
                "Figma request rate limit reached.",
                true,
            ),
            Self::NodeNotFound => (
                ErrorCode::DevupFigmaNodeNotFound,
                "Figma node not found.",
                false,
            ),
            Self::VersionChanged => (
                ErrorCode::DevupFigmaVersionChanged,
                "The Figma file version changed during collection.",
                true,
            ),
            Self::Transport => (
                ErrorCode::DevupFigmaDirectUnavailable,
                "Failed to complete the Figma direct connection.",
                true,
            ),
            Self::InvalidResponse => (
                ErrorCode::DevupSnapshotUnsupported,
                "Failed to safely interpret the Figma MCP response.",
                false,
            ),
        };
        let mut details = json!({ "source": "direct", "status": status });
        if self == Self::CatalogRejected {
            details["options"] = json!([
                "Register devup-mcp on the Figma MCP Catalog waitlist: https://www.figma.com/mcp-catalog/",
                "Inject client credentials you obtained yourself via devup_figma_auth { action: \"configure\", clientId, clientSecret }",
                "Hand off to the official Figma MCP registered on the host (sourcePolicy: auto or host, the current default fallback)"
            ]);
        }
        DevupError::with_details(code, message, retryable, details)
    }
}
