use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::{DevupError, ErrorCode, SecretString};

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

    pub fn probe() -> Result<(), DevupError> {
        Self::entry().map(|_| ())
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

/// A user-supplied, pre-registered Figma Remote MCP OAuth client (see
/// `README.md`'s "Figma 연결 설정" section for why devup-mcp cannot
/// register its own client). devup-mcp never invents this value: it is
/// only ever accepted from `--figma-client-id`/`--figma-client-secret`,
/// `DEVUP_FIGMA_CLIENT_ID`/`DEVUP_FIGMA_CLIENT_SECRET`, or the
/// `devup_figma_auth {"action":"configure"}` tool.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCredentials {
    pub client_id: String,
    pub client_secret: Option<SecretString>,
}

impl std::fmt::Debug for ClientCredentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ClientCredentials")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

/// Persists a user-supplied [`ClientCredentials`] so it survives process
/// restarts, independent of the OAuth token stored in [`CredentialStore`].
#[async_trait]
pub trait ClientCredentialStore: Send + Sync + 'static {
    async fn load(&self) -> Result<Option<ClientCredentials>, DevupError>;
    async fn save(&self, value: &ClientCredentials) -> Result<(), DevupError>;
    async fn clear(&self) -> Result<(), DevupError>;
}

#[derive(Clone, Default)]
pub struct MemoryClientCredentialStore {
    value: Arc<RwLock<Option<ClientCredentials>>>,
}

#[async_trait]
impl ClientCredentialStore for MemoryClientCredentialStore {
    async fn load(&self) -> Result<Option<ClientCredentials>, DevupError> {
        Ok(self.value.read().await.clone())
    }

    async fn save(&self, value: &ClientCredentials) -> Result<(), DevupError> {
        *self.value.write().await = Some(value.clone());
        Ok(())
    }

    async fn clear(&self) -> Result<(), DevupError> {
        *self.value.write().await = None;
        Ok(())
    }
}

/// OS credential store backend for [`ClientCredentialStore`]. Uses a
/// distinct keyring entry from [`KeyringCredentialStore`] (which holds the
/// OAuth token) so configuring a client credential never touches the
/// stored access/refresh token, and vice versa.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyringClientCredentialStore;

impl KeyringClientCredentialStore {
    fn entry() -> Result<keyring::Entry, DevupError> {
        keyring::Entry::new("devup-mcp", "figma-client-credentials").map_err(keyring_error)
    }
}

#[async_trait]
impl ClientCredentialStore for KeyringClientCredentialStore {
    async fn load(&self) -> Result<Option<ClientCredentials>, DevupError> {
        tokio::task::spawn_blocking(|| match Self::entry()?.get_password() {
            Ok(json) => serde_json::from_str(&json).map(Some).map_err(|_| {
                DevupError::new(
                    ErrorCode::DevupAuthRequired,
                    "저장된 Figma client 자격증명을 읽을 수 없습니다. 다시 configure하세요.",
                    false,
                )
            }),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(keyring_error(error)),
        })
        .await
        .map_err(|_| credential_task_error())?
    }

    async fn save(&self, value: &ClientCredentials) -> Result<(), DevupError> {
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
