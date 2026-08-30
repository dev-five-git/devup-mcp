use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::{DevupError, ErrorCode};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredAuthorization {
    pub client_id: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<u64>,
    pub scope: String,
    pub token_endpoint: String,
    pub resource: String,
}

impl std::fmt::Debug for StoredAuthorization {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredAuthorization")
            .field("client_id", &self.client_id)
            .field("access_token", &"[REDACTED]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("expires_at", &self.expires_at)
            .field("scope", &self.scope)
            .field("token_endpoint", &self.token_endpoint)
            .field("resource", &self.resource)
            .finish()
    }
}

#[async_trait]
pub trait CredentialStore: Clone + Send + Sync + 'static {
    async fn load(&self) -> Result<Option<StoredAuthorization>, DevupError>;
    async fn save(&self, value: &StoredAuthorization) -> Result<(), DevupError>;
    async fn clear(&self) -> Result<(), DevupError>;
}

#[derive(Clone, Default)]
pub struct MemoryCredentialStore {
    value: Arc<RwLock<Option<StoredAuthorization>>>,
}

#[async_trait]
impl CredentialStore for MemoryCredentialStore {
    async fn load(&self) -> Result<Option<StoredAuthorization>, DevupError> {
        Ok(self.value.read().await.clone())
    }

    async fn save(&self, value: &StoredAuthorization) -> Result<(), DevupError> {
        *self.value.write().await = Some(value.clone());
        Ok(())
    }

    async fn clear(&self) -> Result<(), DevupError> {
        *self.value.write().await = None;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct KeyringCredentialStore;

impl KeyringCredentialStore {
    fn entry() -> Result<keyring::Entry, DevupError> {
        keyring::Entry::new("devup-mcp", "figma-remote-mcp").map_err(keyring_error)
    }
}

#[async_trait]
impl CredentialStore for KeyringCredentialStore {
    async fn load(&self) -> Result<Option<StoredAuthorization>, DevupError> {
        tokio::task::spawn_blocking(|| match Self::entry()?.get_password() {
            Ok(json) => serde_json::from_str(&json).map(Some).map_err(|_| {
                DevupError::new(
                    ErrorCode::DevupAuthRequired,
                    "저장된 Figma 인증 정보를 읽을 수 없습니다. 다시 로그인하세요.",
                    false,
                )
            }),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(keyring_error(error)),
        })
        .await
        .map_err(|_| credential_task_error())?
    }

    async fn save(&self, value: &StoredAuthorization) -> Result<(), DevupError> {
        let json = serde_json::to_string(value).map_err(|_| credential_task_error())?;
        tokio::task::spawn_blocking(move || {
            Self::entry()?.set_password(&json).map_err(keyring_error)
        })
        .await
        .map_err(|_| credential_task_error())?
    }

    async fn clear(&self) -> Result<(), DevupError> {
        tokio::task::spawn_blocking(|| match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(keyring_error(error)),
        })
        .await
        .map_err(|_| credential_task_error())?
    }
}

fn keyring_error(_error: keyring::Error) -> DevupError {
    DevupError::new(
        ErrorCode::DevupAuthRequired,
        "운영체제 보안 저장소에 Figma 인증 정보를 저장할 수 없습니다.",
        false,
    )
}

fn credential_task_error() -> DevupError {
    DevupError::new(
        ErrorCode::DevupAuthRequired,
        "Figma 인증 저장소 작업을 완료하지 못했습니다.",
        true,
    )
}
