use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    DevupAuthRequired,
    DevupAuthCallbackTimeout,
    DevupAuthStateMismatch,
    DevupFigmaCallbackPortInUse,
    DevupFigmaPermissionDenied,
    DevupFigmaRateLimited,
    DevupFigmaDirectUnavailable,
    DevupFigmaCatalogRejected,
    DevupFigmaHandoffExpired,
    DevupFigmaHandoffInvalid,
    DevupFigmaNodeNotFound,
    DevupFigmaUnsupportedFile,
    DevupFigmaResponseTooLarge,
    DevupFigmaVersionChanged,
    DevupSnapshotUnsupported,
    DevupCodegenFailed,
    DevupThemeConflict,
    DevupInvalidInput,
    DevupProjectRootNotFound,
}

impl ErrorCode {
    /// Whether this names a mistake in the call itself rather than a failure
    /// behind it.
    ///
    /// Every error used to reach the caller as JSON-RPC `INTERNAL_ERROR`, so
    /// "you passed a scope this tool does not have" and "Figma stopped
    /// answering" arrived indistinguishable at the protocol level. That
    /// matters more here than in a human-facing API, because the caller is
    /// usually an agent deciding between two different next moves: fix the
    /// arguments and call again, or stop and report. The codes below are the
    /// ones the caller can act on by changing what it sent.
    pub const fn is_caller_mistake(self) -> bool {
        matches!(
            self,
            Self::DevupInvalidInput
                | Self::DevupFigmaNodeNotFound
                | Self::DevupFigmaUnsupportedFile
                | Self::DevupProjectRootNotFound
                | Self::DevupFigmaHandoffInvalid
                | Self::DevupFigmaHandoffExpired
        )
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DevupError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    pub details: serde_json::Value,
}

impl DevupError {
    pub fn new(code: ErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
            details: serde_json::Value::Null,
        }
    }

    pub fn unsupported_file(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::DevupFigmaUnsupportedFile, message, false)
    }

    pub fn with_details(
        code: ErrorCode,
        message: impl Into<String>,
        retryable: bool,
        details: serde_json::Value,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
            details,
        }
    }
}

impl std::fmt::Debug for DevupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DevupError")
            .field("code", &self.code)
            .field("message", &self.message)
            .field("retryable", &self.retryable)
            .field("details", &self.details)
            .finish()
    }
}

impl std::fmt::Display for DevupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DevupError {}
