use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

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

use super::{
    ClientCredentialStore, ClientCredentials, CredentialStore, DevupError, ErrorCode,
    MemoryClientCredentialStore, StoredAuthorization, UpstreamFailureContext,
    upstream_failure_error,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthStatus {
    Connected,
    Disconnected,
}

/// Where a resolved [`ClientCredentials`] came from, reported by
/// `devup_figma_auth {"action":"doctor"}` so an agent (or human) can tell
/// *why* a particular client is in play without ever seeing the secret
/// itself. See `README.md`'s "Figma 연결 설정" for the three supported
/// injection paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClientCredentialSource {
    CliArg,
    Env,
    CredentialStore,
    #[default]
    None,
}

/// Freshness of the OAuth token in the [`CredentialStore`], independent of
/// whether a [`ClientCredentials`] is configured. `Expired` still means a
/// refresh is possible if a `refresh_token` was stored; it does not by
/// itself make `direct` unavailable (see `AuthStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TokenState {
    Valid,
    Expired,
    Absent,
}

/// Everything `doctor` needs to describe the `direct` connection path
/// without ever including the client secret or access/refresh tokens
/// themselves — only their provenance and state.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectPathSnapshot {
    pub credential_source: ClientCredentialSource,
    pub token_state: TokenState,
    pub callback_port: Option<u16>,
    pub callback_port_free: Option<bool>,
    /// The `client_name` Dynamic Client Registration would send right
    /// now. Reported because Figma gates `/register` on this exact
    /// string, so a 403 is otherwise indistinguishable from a network
    /// fault. Never a secret — see [`OAuthManager::with_client_name`].
    pub client_name: String,
}

/// The `client_name` devup-mcp sends to Dynamic Client Registration
/// unless the operator overrides it.
///
/// Figma admits `POST /v1/oauth/mcp/register` only for `client_name`
/// values on its catalog allowlist and rejects everything else with a
/// plain-text `403 Forbidden`. devup-mcp itself is not on that
/// allowlist, so the literal name `devup-mcp` makes the `direct` path
/// unreachable. This default is therefore `Codex` — the host devup-mcp
/// is distributed to be installed into — so a Codex install can complete
/// `login` without extra flags.
///
/// Two consequences to be aware of, neither of which devup-mcp can
/// resolve on its own: the value is sent verbatim as this client's
/// identity, so Figma attributes the registration and the resulting
/// traffic to Codex rather than to devup-mcp; and the allowlist is
/// Figma's access control, so this default routes around it. The
/// sanctioned path is admission through
/// <https://www.figma.com/mcp-catalog/>, after which
/// [`OAuthManager::with_client_name`] should carry your own registered
/// name instead.
pub const DEFAULT_CLIENT_NAME: &str = "Codex";

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
                "Could not open the browser. Retry Figma authentication.",
                true,
            )
        })?;
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

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
    callback_port: Option<u16>,
    /// A cli-arg/env-supplied override. Always wins over
    /// `client_credential_store` when present; its `ClientCredentialSource`
    /// is always `CliArg` or `Env`.
    static_client_credentials: Option<(ClientCredentials, ClientCredentialSource)>,
    client_credential_store: Arc<dyn ClientCredentialStore>,
    /// The `client_name` sent to Dynamic Client Registration. Defaults to
    /// [`DEFAULT_CLIENT_NAME`]; overridable per process because Figma
    /// admits `/register` only for allowlisted names.
    client_name: String,
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
            callback_port: None,
            static_client_credentials: None,
            client_credential_store: Arc::new(MemoryClientCredentialStore::default()),
            client_name: DEFAULT_CLIENT_NAME.to_owned(),
        }
    }

    pub fn with_callback_timeout(mut self, timeout: Duration) -> Self {
        self.callback_timeout = timeout;
        self
    }

    /// Fixes the local OAuth callback listener to a specific port instead
    /// of letting the OS assign a free one. Required when a pre-registered
    /// client's `redirect_uri` was registered with an exact port. `None`
    /// (the default) preserves the pre-existing OS-assigned-port behavior.
    pub fn with_callback_port(mut self, port: Option<u16>) -> Self {
        self.callback_port = port;
        self
    }

    /// Overrides the `client_name` sent to Dynamic Client Registration
    /// (default [`DEFAULT_CLIENT_NAME`], i.e. `Codex`).
    ///
    /// Set this to the name your own client was admitted under through
    /// <https://www.figma.com/mcp-catalog/>; doing so stops attributing
    /// this client's registration and traffic to Codex, and is the only
    /// configuration that does not depend on Figma's allowlist gate
    /// staying permissive for a name that is not yours.
    ///
    /// The value is transmitted verbatim to the upstream authorization
    /// server as this client's identity, so whichever name is active is
    /// the identity Figma records. [`Self::direct_path_snapshot`] always
    /// reports the value in play, and never a secret.
    ///
    /// An empty or whitespace-only name is ignored, keeping the default.
    pub fn with_client_name(mut self, client_name: impl Into<String>) -> Self {
        let client_name = client_name.into();
        if !client_name.trim().is_empty() {
            self.client_name = client_name;
        }
        self
    }

    /// Installs a cli-arg/env-supplied client credential override. This
    /// always takes priority over anything in `client_credential_store`,
    /// and causes `login` to skip Dynamic Client Registration entirely.
    pub fn with_static_client_credentials(
        mut self,
        credentials: ClientCredentials,
        source: ClientCredentialSource,
    ) -> Self {
        self.static_client_credentials = Some((credentials, source));
        self
    }

    /// Installs the backend used to persist client credentials configured
    /// via [`Self::configure_client_credentials`]. Defaults to an
    /// in-process-only store so `configure` still works without explicit
    /// wiring in tests; production code should pass a
    /// `KeyringClientCredentialStore`.
    pub fn with_client_credential_store(mut self, store: Arc<dyn ClientCredentialStore>) -> Self {
        self.client_credential_store = store;
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

    /// Persists a user-supplied client credential (from the
    /// `devup_figma_auth {"action":"configure"}` tool) so subsequent
    /// `login` calls skip Dynamic Client Registration, even across process
    /// restarts, without requiring `--figma-client-id`/`DEVUP_FIGMA_CLIENT_ID`
    /// on every launch.
    pub async fn configure_client_credentials(
        &self,
        client_id: String,
        client_secret: Option<String>,
    ) -> Result<(), DevupError> {
        let credentials = ClientCredentials {
            client_id,
            client_secret: client_secret.map(SecretString),
        };
        self.client_credential_store.save(&credentials).await
    }

    /// Resolves the client credential that `login`/`refresh` should use,
    /// in priority order: cli-arg/env override, then the persisted
    /// client-credential store, then `None` (Dynamic Client Registration).
    async fn resolve_client_credentials(
        &self,
    ) -> Result<Option<(ClientCredentials, ClientCredentialSource)>, DevupError> {
        if let Some((credentials, source)) = &self.static_client_credentials {
            return Ok(Some((credentials.clone(), *source)));
        }
        if let Some(credentials) = self.client_credential_store.load().await? {
            return Ok(Some((credentials, ClientCredentialSource::CredentialStore)));
        }
        Ok(None)
    }

    async fn token_state(&self) -> Result<TokenState, DevupError> {
        Ok(match self.store.load().await? {
            None => TokenState::Absent,
            Some(authorization) => match authorization.expires_at {
                Some(expires_at) if expires_at <= now() => TokenState::Expired,
                _ => TokenState::Valid,
            },
        })
    }

    /// Builds the `paths.direct` snapshot for `devup_figma_auth
    /// {"action":"doctor"}`: which credential is in play (never the secret
    /// itself), whether the stored token is still fresh, and — when a
    /// fixed callback port is configured — whether it is actually free
    /// right now (measured, not assumed).
    pub async fn direct_path_snapshot(&self) -> Result<DirectPathSnapshot, DevupError> {
        let credential_source = self
            .resolve_client_credentials()
            .await?
            .map(|(_, source)| source)
            .unwrap_or_default();
        let token_state = self.token_state().await?;
        let callback_port_free = match self.callback_port {
            Some(port) => Some(probe_callback_port_free(port).await),
            None => None,
        };
        Ok(DirectPathSnapshot {
            credential_source,
            token_state,
            callback_port: self.callback_port,
            callback_port_free,
            client_name: self.client_name.clone(),
        })
    }

    pub async fn login(
        &self,
        opener: &dyn BrowserOpener,
    ) -> Result<StoredAuthorization, DevupError> {
        let metadata = self.discover().await?;
        let listener = bind_callback_listener(self.callback_port).await?;
        let redirect_uri = format!(
            "http://127.0.0.1:{}/callback",
            listener.local_addr().map_err(callback_error)?.port()
        );

        // A resolved client credential (cli-arg/env override or a
        // previously `configure`d value) always skips Dynamic Client
        // Registration. Otherwise devup-mcp registers under
        // `self.client_name` — `DEFAULT_CLIENT_NAME` (`Codex`) unless
        // `--figma-client-name`/`DEVUP_FIGMA_CLIENT_NAME` supplied the
        // name this deployment was actually admitted under — and Figma's
        // allowlist decides the outcome. See `DEFAULT_CLIENT_NAME` for
        // what that default does and does not license.
        let resolved = self.resolve_client_credentials().await?;
        let (client_id, client_secret) = match resolved {
            Some((credentials, _source)) => (credentials.client_id, credentials.client_secret),
            None => {
                let response = self
                    .client
                    .post(&metadata.registration_endpoint)
                    .json(&serde_json::json!({
                        "client_name": self.client_name.as_str(),
                        "redirect_uris": [redirect_uri],
                        "grant_types": ["authorization_code", "refresh_token"],
                        "response_types": ["code"],
                        "token_endpoint_auth_method": "none",
                        "application_type": "native",
                        "scope": "mcp:connect"
                    }))
                    .send()
                    .await
                    .map_err(auth_network_error)?;
                let status = response.status();
                if !status.is_success() {
                    let body = response.text().await.unwrap_or_default();
                    return Err(upstream_failure_error(
                        UpstreamFailureContext::RegisterClient,
                        Some(status.as_u16()),
                        &body,
                    ));
                }
                let registration: RegistrationResponse =
                    response.json().await.map_err(auth_network_error)?;
                (
                    registration.client_id,
                    registration.client_secret.map(SecretString),
                )
            }
        };

        let state = random_urlsafe(32);
        let verifier = random_urlsafe(64);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut authorization_url =
            Url::parse(&metadata.authorization_endpoint).map_err(|_| invalid_metadata())?;
        authorization_url
            .query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("scope", "mcp:connect")
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("resource", metadata.resource.as_str());

        opener.open(authorization_url.as_str())?;
        let callback = receive_callback(listener, &state, self.callback_timeout).await?;
        let mut form: Vec<(&str, &str)> = vec![
            ("grant_type", "authorization_code"),
            ("client_id", client_id.as_str()),
            ("code", callback.code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("code_verifier", verifier.as_str()),
            ("resource", metadata.resource.as_str()),
        ];
        if let Some(secret) = client_secret.as_ref() {
            form.push(("client_secret", secret.expose()));
        }
        let token: TokenResponse = self
            .client
            .post(&metadata.token_endpoint)
            .form(&form)
            .send()
            .await
            .map_err(auth_network_error)?
            .error_for_status()
            .map_err(auth_network_error)?
            .json()
            .await
            .map_err(auth_network_error)?;

        let authorization = StoredAuthorization {
            client_id,
            client_secret,
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
        let resolved = self.resolve_client_credentials().await?;
        let mut form: Vec<(&str, &str)> = vec![
            ("grant_type", "refresh_token"),
            ("client_id", authorization.client_id.as_str()),
            ("refresh_token", refresh_token.as_str()),
            ("resource", authorization.resource.as_str()),
        ];
        // The secret that belongs to *this* authorization wins: when the
        // client was registered through DCR the operator has no configured
        // credential at all, and dropping it here would fail the refresh with
        // the same bare 400 the initial exchange used to.
        if let Some(secret) = authorization.client_secret.as_ref().or_else(|| {
            resolved
                .as_ref()
                .and_then(|(credentials, _source)| credentials.client_secret.as_ref())
        }) {
            form.push(("client_secret", secret.expose()));
        }
        let response: TokenResponse = self
            .client
            .post(&authorization.token_endpoint)
            .form(&form)
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
    /// Figma's authorization server advertises only `client_secret_basic`
    /// and `client_secret_post`, so its Dynamic Client Registration response
    /// issues a secret and every subsequent token/refresh request must send
    /// it. Discarding this field made the authorization-code exchange fail
    /// with a bare `400` from `/v1/oauth/token` after an otherwise fully
    /// successful registration and browser consent. `Option` because an
    /// authorization server that genuinely supports public clients
    /// (`token_endpoint_auth_method: none`) omits it.
    #[serde(default)]
    client_secret: Option<String>,
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
                "Figma authentication response timed out.",
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
            "Figma authentication state validation failed.",
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
        "Figma authentication is complete. You can close this window."
    } else {
        "Figma authentication could not be verified. Try again."
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

/// Binds the local OAuth callback listener. When `port` is `None`, keeps
/// the pre-existing behavior of letting the OS assign a free ephemeral
/// port (`0`). When `port` is `Some`, the bind attempt itself is the
/// availability check: a fixed port that is already in use fails
/// immediately with [`callback_port_in_use_error`] instead of silently
/// waiting — binding is not retried and no listener that never receives a
/// connection is created.
async fn bind_callback_listener(port: Option<u16>) -> Result<TcpListener, DevupError> {
    let requested_port = port.unwrap_or(0);
    TcpListener::bind(("127.0.0.1", requested_port))
        .await
        .map_err(|error| match port {
            Some(configured_port) => callback_port_in_use_error(configured_port, error),
            None => callback_error(error),
        })
}

/// Best-effort probe for `doctor`: attempts to bind `port` and immediately
/// releases it. `true` means the port was free at the moment of the probe
/// (not a guarantee it stays free); `false` means something is already
/// listening there. Never blocks waiting for a connection.
pub async fn probe_callback_port_free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).await.is_ok()
}

fn callback_port_in_use_error(port: u16, _error: std::io::Error) -> DevupError {
    DevupError::with_details(
        ErrorCode::DevupFigmaCallbackPortInUse,
        format!(
            "The configured Figma auth callback port {port} is already in use by another \
             process. If the OS or security software holds this port, the browser looks like \
             the redirect succeeded, but the request is delivered to that other process instead \
             of devup-mcp, so authentication never completes. Stop the process holding the port, \
             or pick a different port with --figma-callback-port."
        ),
        false,
        serde_json::json!({ "port": port }),
    )
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
        "Figma authentication is required.",
        false,
    )
}

fn invalid_metadata() -> DevupError {
    DevupError::new(
        ErrorCode::DevupAuthRequired,
        "Cannot validate the Figma OAuth server metadata.",
        false,
    )
}

/// Classifies a transport failure against the Figma OAuth server.
///
/// The cause used to be discarded outright, which made every failure — DNS,
/// a TLS trust failure behind a corporate proxy, a timeout, a malformed
/// metadata document — surface as the same opaque sentence with
/// `details: null`, leaving no way to tell them apart. The details below are
/// derived from the error itself and its source chain; the URL is reduced to
/// scheme/host/path so a query string can never carry an authorization code
/// or token into a log.
fn auth_network_error(error: reqwest::Error) -> DevupError {
    let kind = if error.is_connect() {
        "connect"
    } else if error.is_timeout() {
        "timeout"
    } else if error.is_decode() {
        "decode"
    } else if error.is_status() {
        "status"
    } else if error.is_body() {
        "body"
    } else if error.is_redirect() {
        "redirect"
    } else if error.is_request() {
        "request"
    } else {
        "unknown"
    };
    let mut causes = Vec::new();
    let mut source = std::error::Error::source(&error);
    while let Some(current) = source {
        causes.push(current.to_string());
        source = current.source();
    }
    DevupError::with_details(
        ErrorCode::DevupAuthRequired,
        "Failed to communicate with the Figma OAuth server.",
        true,
        serde_json::json!({
            "kind": kind,
            "status": error.status().map(|status| status.as_u16()),
            "url": error.url().map(|url| {
                format!("{}://{}{}", url.scheme(), url.host_str().unwrap_or(""), url.path())
            }),
            "causes": causes,
        }),
    )
}

fn callback_error(_error: std::io::Error) -> DevupError {
    DevupError::new(
        ErrorCode::DevupAuthRequired,
        "Failed to handle the local Figma authentication callback.",
        true,
    )
}
