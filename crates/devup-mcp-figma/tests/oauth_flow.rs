use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::{Form, State},
    routing::{get, post},
};
use devup_mcp_figma::{
    AuthStatus, BrowserOpener, CredentialStore, MemoryCredentialStore, OAuthManager,
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
    assert_eq!(registration["client_name"], "devup-mcp");
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
