use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::{Form, State},
    http::StatusCode,
    routing::{get, post},
};
use devup_mcp_figma::{
    AuthStatus, BrowserOpener, ClientCredentialSource, ClientCredentials, CredentialStore,
    DEFAULT_CLIENT_NAME, DirectPathSnapshot, ErrorCode, MemoryClientCredentialStore,
    MemoryCredentialStore, OAuthManager, SecretString, TokenState,
};
use serde_json::{Value, json};
use tokio::{net::TcpListener, sync::Mutex};

#[derive(Clone, Default)]
struct Captured {
    registration: Arc<Mutex<Option<Value>>>,
    token_form: Arc<Mutex<Option<HashMap<String, String>>>>,
}

#[derive(Clone)]
struct AppState {
    base: String,
    captured: Captured,
}

async fn protected_resource(State(state): State<AppState>) -> Json<Value> {
    let base = state.base;
    Json(json!({
        "resource": format!("{base}/mcp"),
        "authorization_servers": [base],
        "scopes_supported": ["mcp:connect"]
    }))
}

async fn authorization_metadata(State(state): State<AppState>) -> Json<Value> {
    let base = state.base;
    Json(json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/authorize"),
        "token_endpoint": format!("{base}/token"),
        "registration_endpoint": format!("{base}/register"),
        "code_challenge_methods_supported": ["S256"],
        "scopes_supported": ["mcp:connect"]
    }))
}

async fn register(State(state): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    *state.captured.registration.lock().await = Some(body);
    Json(json!({"client_id": "dynamic-client"}))
}

async fn token(
    State(state): State<AppState>,
    Form(form): Form<HashMap<String, String>>,
) -> Json<Value> {
    *state.captured.token_form.lock().await = Some(form);
    Json(json!({
        "access_token": "access-secret",
        "refresh_token": "refresh-secret",
        "token_type": "Bearer",
        "expires_in": 3600,
        "scope": "mcp:connect"
    }))
}

#[derive(Debug)]
struct CallbackOpener;

impl BrowserOpener for CallbackOpener {
    fn open(&self, authorization_url: &str) -> Result<(), devup_mcp_figma::DevupError> {
        let url = url::Url::parse(authorization_url).expect("authorization URL");
        let values = url.query_pairs().into_owned().collect::<HashMap<_, _>>();
        let redirect = values.get("redirect_uri").expect("redirect_uri").clone();
        let state = values.get("state").expect("state").clone();
        tokio::spawn(async move {
            reqwest::get(format!("{redirect}?code=authorization-code&state={state}"))
                .await
                .expect("callback request");
        });
        Ok(())
    }
}

#[tokio::test]
async fn login_discovers_registers_uses_pkce_and_stores_tokens() -> anyhow::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let captured = Captured::default();
    let app = Router::new()
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(protected_resource),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(authorization_metadata),
        )
        .route("/register", post(register))
        .route("/token", post(token))
        .with_state(AppState {
            base: base.clone(),
            captured: captured.clone(),
        });
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("mock OAuth server");
    });

    let store = MemoryCredentialStore::default();
    let manager = OAuthManager::with_endpoint(format!("{base}/mcp"), store.clone())
        .with_callback_timeout(Duration::from_secs(3));
    let authorization = manager.login(&CallbackOpener).await?;

    assert_eq!(manager.status().await?, AuthStatus::Connected);
    assert_eq!(manager.access_token().await?.expose(), "access-secret");
    assert!(!format!("{authorization:?}").contains("access-secret"));
    assert!(!format!("{authorization:?}").contains("refresh-secret"));

    let registration = captured
        .registration
        .lock()
        .await
        .clone()
        .expect("registration");
    assert_eq!(registration["client_name"], DEFAULT_CLIENT_NAME);
    assert_eq!(registration["token_endpoint_auth_method"], "none");
    assert!(
        registration["redirect_uris"][0]
            .as_str()
            .expect("redirect uri")
            .starts_with("http://127.0.0.1:")
    );

    let form = captured
        .token_form
        .lock()
        .await
        .clone()
        .expect("token form");
    assert_eq!(
        form.get("client_id").map(String::as_str),
        Some("dynamic-client")
    );
    assert_eq!(
        form.get("code").map(String::as_str),
        Some("authorization-code")
    );
    assert!(
        form.get("code_verifier")
            .is_some_and(|value| value.len() >= 43)
    );
    assert_eq!(
        form.get("resource").map(String::as_str),
        Some(format!("{base}/mcp").as_str())
    );
    Ok(())
}

#[tokio::test]
async fn logout_clears_persisted_authorization() -> anyhow::Result<()> {
    let store = MemoryCredentialStore::default();
    assert!(CredentialStore::load(&store).await?.is_none());
    let manager = OAuthManager::with_endpoint("https://mcp.figma.com/mcp", store);
    manager.logout().await?;
    assert_eq!(manager.status().await?, AuthStatus::Disconnected);
    Ok(())
}

/// Mirrors Figma's real Dynamic Client Registration: its authorization
/// server advertises only `client_secret_basic`/`client_secret_post`, so
/// registration issues a confidential client with a secret.
async fn register_confidential(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Json<Value> {
    *state.captured.registration.lock().await = Some(body);
    Json(json!({"client_id": "dynamic-client", "client_secret": "dynamic-secret"}))
}

/// Regression: a secret issued by Dynamic Client Registration must reach the
/// authorization-code exchange. Discarding it made every real Figma login
/// fail with a bare `400` from `/v1/oauth/token` — after registration and
/// browser consent had both already succeeded, which made the failure look
/// like a network fault rather than a missing credential.
#[tokio::test]
async fn a_dcr_issued_client_secret_is_used_for_the_token_exchange_and_refresh()
-> anyhow::Result<()> {
    let (base, captured) = spawn_mock_oauth_server(post(register_confidential)).await?;

    let store = MemoryCredentialStore::default();
    let manager = OAuthManager::with_endpoint(format!("{base}/mcp"), store.clone())
        .with_callback_timeout(Duration::from_secs(3));
    let authorization = manager.login(&CallbackOpener).await?;

    let form = captured
        .token_form
        .lock()
        .await
        .clone()
        .expect("token form");
    assert_eq!(
        form.get("client_secret").map(String::as_str),
        Some("dynamic-secret"),
        "the DCR-issued secret must be sent to the token endpoint"
    );

    // It is kept with the authorization it belongs to, so a later refresh —
    // which has no operator-configured credential to fall back on — can send
    // it too. The secret must never surface in Debug output.
    assert_eq!(
        authorization
            .client_secret
            .as_ref()
            .map(SecretString::expose),
        Some("dynamic-secret")
    );
    assert!(!format!("{authorization:?}").contains("dynamic-secret"));

    // Force the stored token to look expired so `access_token` refreshes.
    let mut expired = CredentialStore::load(&store).await?.expect("authorization");
    expired.expires_at = Some(0);
    CredentialStore::save(&store, &expired).await?;
    manager.access_token().await?;
    let refresh_form = captured
        .token_form
        .lock()
        .await
        .clone()
        .expect("refresh form");
    assert_eq!(
        refresh_form.get("grant_type").map(String::as_str),
        Some("refresh_token")
    );
    assert_eq!(
        refresh_form.get("client_secret").map(String::as_str),
        Some("dynamic-secret"),
        "refresh must carry the DCR-issued secret as well"
    );
    Ok(())
}

async fn register_forbidden(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> (StatusCode, String) {
    *state.captured.registration.lock().await = Some(body);
    // Real Figma returns a *plain-text* 403 body, not JSON — this is the
    // exact shape that broke naive OAuth clients (see README.md). The
    // fixture reproduces it so tests exercise the real failure mode.
    (StatusCode::FORBIDDEN, "Forbidden".to_owned())
}

async fn spawn_mock_oauth_server(
    register: axum::routing::MethodRouter<AppState>,
) -> anyhow::Result<(String, Captured)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let captured = Captured::default();
    let app = Router::new()
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(protected_resource),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(authorization_metadata),
        )
        .route("/register", register)
        .route("/token", post(token))
        .with_state(AppState {
            base: base.clone(),
            captured: captured.clone(),
        });
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("mock OAuth server");
    });
    Ok((base, captured))
}

/// Core deliverable #1: when a pre-registered client credential is
/// resolvable (here via `with_static_client_credentials`, standing in for
/// `--figma-client-id`/`DEVUP_FIGMA_CLIENT_ID`), `login` must skip
/// Dynamic Client Registration entirely — the `/register` endpoint must
/// never be called — and use the given `client_id`/`client_secret` for the
/// PKCE authorization-code exchange.
#[tokio::test]
async fn static_client_credentials_skip_dynamic_client_registration() -> anyhow::Result<()> {
    let (base, captured) = spawn_mock_oauth_server(post(register)).await?;

    let store = MemoryCredentialStore::default();
    let manager = OAuthManager::with_endpoint(format!("{base}/mcp"), store)
        .with_callback_timeout(Duration::from_secs(3))
        .with_static_client_credentials(
            ClientCredentials {
                client_id: "preregistered-client".to_owned(),
                client_secret: Some(SecretString::new("preregistered-secret")),
            },
            ClientCredentialSource::CliArg,
        );
    let authorization = manager.login(&CallbackOpener).await?;

    assert!(
        captured.registration.lock().await.is_none(),
        "DCR must never be attempted once a client credential resolves"
    );
    assert_eq!(authorization.client_id, "preregistered-client");

    let form = captured
        .token_form
        .lock()
        .await
        .clone()
        .expect("token form");
    assert_eq!(
        form.get("client_id").map(String::as_str),
        Some("preregistered-client")
    );
    assert_eq!(
        form.get("client_secret").map(String::as_str),
        Some("preregistered-secret")
    );
    Ok(())
}

/// Core deliverable #3: with no client credential resolvable and no
/// operator-supplied override, `login` performs DCR under
/// `DEFAULT_CLIENT_NAME`, and a
/// 403 rejection (Figma's real response shape: plain-text `Forbidden`, not
/// JSON) surfaces as a classified, actionable `DEVUP_FIGMA_CATALOG_REJECTED`
/// error — not a generic network failure — carrying the four documented
/// options without ever echoing the raw upstream body.
#[tokio::test]
async fn dcr_403_is_classified_as_catalog_rejected_with_actionable_options() -> anyhow::Result<()> {
    let (base, captured) = spawn_mock_oauth_server(post(register_forbidden)).await?;

    let store = MemoryCredentialStore::default();
    let manager = OAuthManager::with_endpoint(format!("{base}/mcp"), store)
        .with_callback_timeout(Duration::from_secs(3));
    let error = manager
        .login(&CallbackOpener)
        .await
        .expect_err("403 registration must fail login");

    assert_eq!(error.code, ErrorCode::DevupFigmaCatalogRejected);
    let options = error.details["options"]
        .as_array()
        .expect("catalog-rejected errors carry actionable options");
    // Three, not four: the local Dev Mode MCP was offered here and cannot
    // serve devup-mcp at all, since it has no use_figma to run a collection
    // with. An option that cannot work costs a turn to discover.
    assert_eq!(options.len(), 3);
    assert!(
        options
            .iter()
            .any(|option| option.as_str().unwrap_or_default().contains("configure"))
    );
    assert!(
        options
            .iter()
            .any(|option| option.as_str().unwrap_or_default().contains("mcp-catalog"))
    );
    let serialized = serde_json::to_string(&error)?;
    assert!(!serialized.contains("Forbidden"));

    // Confirm the request that actually went out carried the compiled
    // default, so a 403 here is attributable to the allowlist rather than
    // to a stray per-process override.
    let registration = captured
        .registration
        .lock()
        .await
        .clone()
        .expect("registration attempt");
    assert_eq!(registration["client_name"], DEFAULT_CLIENT_NAME);
    Ok(())
}

/// The compiled default is a deployment decision, not an implementation
/// detail: devup-mcp is distributed to be installed into Codex, and the
/// literal name `devup-mcp` is not on Figma's catalog allowlist, so
/// defaulting to it would make `direct` unreachable out of the box. Pin
/// the value so flipping it is a deliberate, reviewed edit rather than a
/// silent drift — and pin that the override still wins over it.
#[test]
fn default_client_name_is_codex_and_remains_overridable() {
    assert_eq!(DEFAULT_CLIENT_NAME, "Codex");
}

/// Figma admits `/register` only for `client_name` values on its catalog
/// allowlist, so an operator whose client was admitted under a different
/// name must be able to supply it at launch
/// (`--figma-client-name`/`DEVUP_FIGMA_CLIENT_NAME`) without a rebuild.
/// The override must reach the registration body verbatim — and only the
/// name changes: PKCE, redirect_uri and the token exchange are untouched.
#[tokio::test]
async fn configured_client_name_is_sent_verbatim_to_dynamic_client_registration()
-> anyhow::Result<()> {
    let (base, captured) = spawn_mock_oauth_server(post(register)).await?;

    let store = MemoryCredentialStore::default();
    let manager = OAuthManager::with_endpoint(format!("{base}/mcp"), store)
        .with_callback_timeout(Duration::from_secs(3))
        .with_client_name("Acme Registered Client");
    manager.login(&CallbackOpener).await?;

    let registration = captured
        .registration
        .lock()
        .await
        .clone()
        .expect("registration attempt");
    assert_eq!(registration["client_name"], "Acme Registered Client");
    assert_eq!(registration["token_endpoint_auth_method"], "none");
    assert!(
        registration["redirect_uris"][0]
            .as_str()
            .expect("redirect uri")
            .starts_with("http://127.0.0.1:")
    );

    let snapshot = manager.direct_path_snapshot().await?;
    assert_eq!(snapshot.client_name, "Acme Registered Client");
    Ok(())
}

/// A blank override is operator error (an unset env var expanding to an
/// empty string, say) and must never be sent as the client's identity —
/// it falls back to the honest default instead.
#[tokio::test]
async fn blank_client_name_override_falls_back_to_the_default() -> anyhow::Result<()> {
    let manager = OAuthManager::with_endpoint(
        "https://mcp.figma.com/mcp",
        MemoryCredentialStore::default(),
    )
    .with_client_name("   ");

    let snapshot = manager.direct_path_snapshot().await?;
    assert_eq!(snapshot.client_name, DEFAULT_CLIENT_NAME);
    Ok(())
}

/// Core deliverable #2: a *configured* callback port that is already
/// occupied must fail the bind attempt immediately with a specific,
/// actionable error — never silently wait for a connection that will
/// never arrive (the `MaEPSBroker.exe`-style trap documented in
/// README.md).
#[tokio::test]
async fn occupied_callback_port_fails_immediately_instead_of_waiting() -> anyhow::Result<()> {
    let (base, _captured) = spawn_mock_oauth_server(post(register)).await?;

    // Bind a real listener to claim a genuinely free ephemeral port, then
    // keep it alive so the manager's bind attempt on that exact port
    // fails deterministically.
    let occupier = TcpListener::bind("127.0.0.1:0").await?;
    let occupied_port = occupier.local_addr()?.port();

    let store = MemoryCredentialStore::default();
    // A generous timeout: if the implementation regressed to "wait for a
    // connection", this test would hang for the full duration instead of
    // returning within milliseconds.
    let manager = OAuthManager::with_endpoint(format!("{base}/mcp"), store)
        .with_callback_timeout(Duration::from_secs(120))
        .with_callback_port(Some(occupied_port));

    let started = std::time::Instant::now();
    let error = manager
        .login(&CallbackOpener)
        .await
        .expect_err("bind on an occupied fixed port must fail");
    let elapsed = started.elapsed();

    assert_eq!(error.code, ErrorCode::DevupFigmaCallbackPortInUse);
    assert!(
        !error.retryable,
        "occupied fixed port is not a retry-me error"
    );
    assert_eq!(error.details["port"], occupied_port);
    assert!(
        elapsed < Duration::from_secs(5),
        "must fail immediately on bind, not wait for the callback timeout: took {elapsed:?}"
    );

    drop(occupier);
    Ok(())
}

/// Core deliverable #5 (`doctor`): `direct_path_snapshot` must reflect the
/// real, measured state — which credential source is active, whether the
/// stored token is still fresh, and whether a configured callback port is
/// actually free right now — without ever exposing the secret itself.
#[tokio::test]
async fn direct_path_snapshot_reports_measured_credential_and_port_state() -> anyhow::Result<()> {
    let credential_store = MemoryClientCredentialStore::default();
    let manager = OAuthManager::with_endpoint(
        "https://mcp.figma.com/mcp",
        MemoryCredentialStore::default(),
    )
    .with_client_credential_store(Arc::new(credential_store));

    // Nothing configured yet: no credential, no token, no fixed port.
    let absent = manager.direct_path_snapshot().await?;
    assert_eq!(absent.credential_source, ClientCredentialSource::None);
    assert_eq!(absent.token_state, TokenState::Absent);
    assert_eq!(absent.callback_port, None);
    assert_eq!(absent.callback_port_free, None);

    // `configure` persists a client credential; its source must now read
    // "credential-store" (not "cli-arg"/"env" — those are for
    // process-launch overrides only).
    manager
        .configure_client_credentials(
            "configured-client".to_owned(),
            Some("configured-secret".to_owned()),
        )
        .await?;
    let configured = manager.direct_path_snapshot().await?;
    assert_eq!(
        configured.credential_source,
        ClientCredentialSource::CredentialStore
    );
    let serialized = serde_json::to_string(&configured)?;
    assert!(!serialized.contains("configured-secret"));

    Ok(())
}

/// `doctor`'s callback-port probe must reflect the real bind state: free
/// when unoccupied, occupied when another listener holds the exact port.
#[tokio::test]
async fn direct_path_snapshot_probes_the_real_callback_port_state() -> anyhow::Result<()> {
    let manager = OAuthManager::with_endpoint(
        "https://mcp.figma.com/mcp",
        MemoryCredentialStore::default(),
    );

    let probe_listener = TcpListener::bind("127.0.0.1:0").await?;
    let free_port = probe_listener.local_addr()?.port();
    drop(probe_listener);
    let free = manager
        .clone()
        .with_callback_port(Some(free_port))
        .direct_path_snapshot()
        .await?;
    assert_eq!(free.callback_port, Some(free_port));
    assert_eq!(free.callback_port_free, Some(true));

    let occupier = TcpListener::bind("127.0.0.1:0").await?;
    let occupied_port = occupier.local_addr()?.port();
    let occupied = manager
        .with_callback_port(Some(occupied_port))
        .direct_path_snapshot()
        .await?;
    assert_eq!(occupied.callback_port_free, Some(false));
    drop(occupier);

    Ok(())
}

/// Security regression: a client secret configured via any path
/// (`with_static_client_credentials` here, standing in for
/// `--figma-client-secret`/`DEVUP_FIGMA_CLIENT_SECRET`) must never appear
/// in `Debug` output of the credential itself or in any snapshot derived
/// from it. `DirectPathSnapshot` structurally has no field capable of
/// carrying it — this test pins that guarantee at the value level too.
#[test]
fn client_secret_never_appears_in_debug_output() {
    let credentials = ClientCredentials {
        client_id: "preregistered-client".to_owned(),
        client_secret: Some(SecretString::new("super-secret-value")),
    };
    let debugged = format!("{credentials:?}");
    assert!(!debugged.contains("super-secret-value"));
    assert!(debugged.contains("REDACTED"));

    let snapshot = DirectPathSnapshot {
        credential_source: ClientCredentialSource::CliArg,
        token_state: TokenState::Valid,
        callback_port: Some(19876),
        callback_port_free: Some(true),
        client_name: DEFAULT_CLIENT_NAME.to_owned(),
    };
    let serialized = serde_json::to_string(&snapshot).expect("snapshot serializes");
    assert!(!serialized.contains("super-secret-value"));
}
