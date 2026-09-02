use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    DevupAuthRequired,
    DevupAuthCallbackTimeout,
    DevupAuthStateMismatch,
    DevupFigmaPermissionDenied,
    DevupFigmaRateLimited,
    DevupFigmaDirectUnavailable,
    DevupFigmaCatalogRejected,
    DevupFigmaHostRequired,
    DevupFigmaHandoffExpired,
    DevupFigmaHandoffInvalid,
    DevupFigmaNodeNotFound,
    DevupFigmaUnsupportedFile,
    DevupFigmaResponseTooLarge,
    DevupFigmaVersionChanged,
    DevupSnapshotUnsupported,
    DevupCodegenFailed,
    DevupThemeConflict,
    DevupCompatCorpusDrift,
    DevupInvalidInput,
    DevupProjectRootNotFound,
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
