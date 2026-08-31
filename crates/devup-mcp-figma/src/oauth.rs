use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use url::Url;

use super::{CredentialStore, DevupError, ErrorCode, StoredAuthorization};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthStatus {
    Connected,
    Disconnected,
}

pub trait BrowserOpener: Send + Sync {
    fn open(&self, authorization_url: &str) -> Result<(), DevupError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemBrowser;

impl BrowserOpener for SystemBrowser {
    fn open(&self, authorization_url: &str) -> Result<(), DevupError> {
        webbrowser::open(authorization_url).map_err(|_| {
            DevupError::new(
                ErrorCode::DevupAuthRequired,
                "브라우저를 열지 못했습니다. Figma 인증을 다시 시도하세요.",
                true,
            )
        })?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct SecretString(String);

impl SecretString {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretString([REDACTED])")
    }
}

#[derive(Clone)]
pub struct OAuthManager<S: CredentialStore> {
    endpoint: Url,
    store: S,
    client: reqwest::Client,
    callback_timeout: Duration,
}

impl<S: CredentialStore> OAuthManager<S> {
    pub fn with_endpoint(endpoint: impl AsRef<str>, store: S) -> Self {
        let endpoint = Url::parse(endpoint.as_ref()).expect("OAuth endpoint must be a valid URL");
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client configuration is valid");
        Self {
            endpoint,
            store,
            client,
            callback_timeout: Duration::from_secs(180),
        }
    }

    pub fn with_callback_timeout(mut self, timeout: Duration) -> Self {
        self.callback_timeout = timeout;
        self
    }

    pub async fn status(&self) -> Result<AuthStatus, DevupError> {
        Ok(if self.store.load().await?.is_some() {
            AuthStatus::Connected
        } else {
            AuthStatus::Disconnected
        })
    }

    pub async fn logout(&self) -> Result<(), DevupError> {
        self.store.clear().await
    }

    pub async fn login(
        &self,
        opener: &dyn BrowserOpener,
    ) -> Result<StoredAuthorization, DevupError> {
        let metadata = self.discover().await?;
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(callback_error)?;
        let redirect_uri = format!(
            "http://127.0.0.1:{}/callback",
            listener.local_addr().map_err(callback_error)?.port()
        );

        let registration: RegistrationResponse = self
            .client
            .post(&metadata.registration_endpoint)
            .json(&serde_json::json!({
                "client_name": "devup-mcp",
                "redirect_uris": [redirect_uri],
                "grant_types": ["authorization_code", "refresh_token"],
                "response_types": ["code"],
                "token_endpoint_auth_method": "none",
                "application_type": "native",
                "scope": "mcp:connect"
            }))
            .send()
            .await
            .map_err(auth_network_error)?
            .error_for_status()
            .map_err(auth_network_error)?
            .json()
            .await
            .map_err(auth_network_error)?;

        let state = random_urlsafe(32);
        let verifier = random_urlsafe(64);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut authorization_url =
            Url::parse(&metadata.authorization_endpoint).map_err(|_| invalid_metadata())?;
        authorization_url
            .query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &registration.client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("scope", "mcp:connect")
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("resource", metadata.resource.as_str());

        opener.open(authorization_url.as_str())?;
        let callback = receive_callback(listener, &state, self.callback_timeout).await?;
        let token: TokenResponse = self
            .client
            .post(&metadata.token_endpoint)
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", registration.client_id.as_str()),
                ("code", callback.code.as_str()),
                ("redirect_uri", redirect_uri.as_str()),
                ("code_verifier", verifier.as_str()),
                ("resource", metadata.resource.as_str()),
            ])
            .send()
            .await
            .map_err(auth_network_error)?
            .error_for_status()
            .map_err(auth_network_error)?
            .json()
            .await
            .map_err(auth_network_error)?;

        let authorization = StoredAuthorization {
            client_id: registration.client_id,
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            expires_at: token
                .expires_in
                .map(|seconds| now().saturating_add(seconds)),
            scope: token.scope.unwrap_or_else(|| "mcp:connect".to_owned()),
            token_endpoint: metadata.token_endpoint,
            resource: metadata.resource.to_string(),
        };
        self.store.save(&authorization).await?;
        Ok(authorization)
    }

    pub async fn access_token(&self) -> Result<SecretString, DevupError> {
        let mut authorization = self.store.load().await?.ok_or_else(auth_required)?;
        if authorization
            .expires_at
            .is_some_and(|expires_at| expires_at <= now().saturating_add(30))
        {
            authorization = self.refresh(authorization).await?;
        }
        Ok(SecretString(authorization.access_token))
    }

    async fn refresh(
        &self,
        mut authorization: StoredAuthorization,
    ) -> Result<StoredAuthorization, DevupError> {
        let refresh_token = authorization
            .refresh_token
            .clone()
            .ok_or_else(auth_required)?;
        let response: TokenResponse = self
            .client
            .post(&authorization.token_endpoint)
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", authorization.client_id.as_str()),
                ("refresh_token", refresh_token.as_str()),
                ("resource", authorization.resource.as_str()),
            ])
            .send()
            .await
            .map_err(auth_network_error)?
            .error_for_status()
            .map_err(|_| auth_required())?
            .json()
            .await
            .map_err(auth_network_error)?;
        authorization.access_token = response.access_token;
        authorization.refresh_token = response.refresh_token.or(authorization.refresh_token);
        authorization.expires_at = response
            .expires_in
            .map(|seconds| now().saturating_add(seconds));
        if let Some(scope) = response.scope {
            authorization.scope = scope;
        }
        self.store.save(&authorization).await?;
        Ok(authorization)
    }

    async fn discover(&self) -> Result<OAuthMetadata, DevupError> {
        let protected_url = protected_resource_url(&self.endpoint);
        let protected: ProtectedResource = self
            .client
            .get(protected_url)
            .send()
            .await
            .map_err(auth_network_error)?
            .error_for_status()
            .map_err(auth_network_error)?
            .json()
            .await
            .map_err(auth_network_error)?;
        if protected.resource != self.endpoint.as_str() {
            return Err(invalid_metadata());
        }
        if protected.authorization_servers.is_empty() {
            return Err(invalid_metadata());
        }

        let metadata_url = authorization_metadata_url(&self.endpoint);
        let metadata: AuthorizationMetadata = self
            .client
            .get(metadata_url)
            .send()
            .await
            .map_err(auth_network_error)?
            .error_for_status()
            .map_err(auth_network_error)?
            .json()
            .await
            .map_err(auth_network_error)?;
        if !metadata
            .code_challenge_methods_supported
            .iter()
            .any(|method| method == "S256")
        {
            return Err(invalid_metadata());
        }
        for endpoint in [
            &metadata.authorization_endpoint,
            &metadata.token_endpoint,
            &metadata.registration_endpoint,
        ] {
            validate_discovered_endpoint(endpoint, self.endpoint.scheme() == "http")?;
        }
        Ok(OAuthMetadata {
            resource: self.endpoint.clone(),
            authorization_endpoint: metadata.authorization_endpoint,
            token_endpoint: metadata.token_endpoint,
            registration_endpoint: metadata.registration_endpoint,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ProtectedResource {
    resource: String,
    #[serde(default)]
    authorization_servers: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AuthorizationMetadata {
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: String,
    #[serde(default)]
    code_challenge_methods_supported: Vec<String>,
}

struct OAuthMetadata {
    resource: Url,
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: String,
}

#[derive(Debug, Deserialize)]
struct RegistrationResponse {
    client_id: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    scope: Option<String>,
}

struct Callback {
    code: String,
}

async fn receive_callback(
    listener: TcpListener,
    expected_state: &str,
    timeout: Duration,
) -> Result<Callback, DevupError> {
    let (mut stream, _) = tokio::time::timeout(timeout, listener.accept())
        .await
        .map_err(|_| {
            DevupError::new(
                ErrorCode::DevupAuthCallbackTimeout,
                "Figma 인증 응답 시간이 초과되었습니다.",
                true,
            )
        })?
        .map_err(callback_error)?;
    let mut request = vec![0_u8; 8192];
    let count = stream.read(&mut request).await.map_err(callback_error)?;
    let request = std::str::from_utf8(&request[..count]).map_err(|_| auth_required())?;
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(auth_required)?;
    let url = Url::parse(&format!("http://127.0.0.1{target}")).map_err(|_| auth_required())?;
    let values = url
        .query_pairs()
        .into_owned()
        .collect::<std::collections::HashMap<_, _>>();
    let state = values.get("state").ok_or_else(auth_required)?;
    if state
        .as_bytes()
        .ct_eq(expected_state.as_bytes())
        .unwrap_u8()
        != 1
    {
        let _ = write_callback_response(&mut stream, false).await;
        return Err(DevupError::new(
            ErrorCode::DevupAuthStateMismatch,
            "Figma 인증 state 검증에 실패했습니다.",
            false,
        ));
    }
    let code = values.get("code").cloned().ok_or_else(auth_required)?;
    write_callback_response(&mut stream, true).await?;
    Ok(Callback { code })
}

async fn write_callback_response(
    stream: &mut tokio::net::TcpStream,
    success: bool,
) -> Result<(), DevupError> {
    let body = if success {
        "Figma 인증이 완료되었습니다. 이 창을 닫아도 됩니다."
    } else {
        "Figma 인증을 확인할 수 없습니다. 다시 시도하세요."
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(callback_error)
}

fn protected_resource_url(endpoint: &Url) -> Url {
    let mut url = endpoint.clone();
    url.set_query(None);
    url.set_fragment(None);
    url.set_path(&format!(
        "/.well-known/oauth-protected-resource{}",
        endpoint.path()
    ));
    url
}

fn authorization_metadata_url(endpoint: &Url) -> Url {
    let mut url = endpoint.clone();
    url.set_query(None);
    url.set_fragment(None);
    url.set_path("/.well-known/oauth-authorization-server");
    url
}

fn validate_discovered_endpoint(endpoint: &str, allow_http: bool) -> Result<(), DevupError> {
    let endpoint = Url::parse(endpoint).map_err(|_| invalid_metadata())?;
    if endpoint.scheme() != "https"
        && !(allow_http && endpoint.scheme() == "http" && endpoint.host_str() == Some("127.0.0.1"))
    {
        return Err(invalid_metadata());
    }
    Ok(())
}

fn random_urlsafe(bytes: usize) -> String {
    let mut value = vec![0_u8; bytes];
    rand::rng().fill_bytes(&mut value);
    URL_SAFE_NO_PAD.encode(value)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn auth_required() -> DevupError {
    DevupError::new(
        ErrorCode::DevupAuthRequired,
        "Figma 인증이 필요합니다.",
        false,
    )
}

fn invalid_metadata() -> DevupError {
    DevupError::new(
        ErrorCode::DevupAuthRequired,
        "Figma OAuth 서버 정보를 검증할 수 없습니다.",
        false,
    )
}

fn auth_network_error(_error: reqwest::Error) -> DevupError {
    DevupError::new(
        ErrorCode::DevupAuthRequired,
        "Figma OAuth 서버와 통신하지 못했습니다.",
        true,
    )
}

fn callback_error(_error: std::io::Error) -> DevupError {
    DevupError::new(
        ErrorCode::DevupAuthRequired,
        "로컬 Figma 인증 callback을 처리하지 못했습니다.",
        true,
    )
}
