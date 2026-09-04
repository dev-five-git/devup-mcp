//! Regression tests for the self-diagnosis surface added to fix the
//! "devup-mcp knows it can't reach Figma but never says so" failure mode:
//! `devup_figma_auth {"action":"doctor"}` and the `hostRequirement` block
//! attached to every `needs_figma` handoff step. See
//! `crates/devup-mcp/src/server/diagnostics.rs`.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use devup_mcp::server::{DevupAuth, DevupServer, Services};
use devup_mcp_figma::{
    AuthStatus, ClientCredentialSource, DEFAULT_CLIENT_NAME, DevupError, DirectPathSnapshot,
    ErrorCode, FigmaUpstream, ReadToolCall, TokenState, UpstreamResult,
};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, CallToolResult},
};
use serde_json::{Map, Value, json};
use tokio::sync::Mutex;

struct AuthProbe {
    status: AuthStatus,
}

#[async_trait]
impl DevupAuth for AuthProbe {
    async fn status(&self) -> Result<AuthStatus, DevupError> {
        Ok(self.status)
    }

    async fn login(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Connected)
    }

    async fn logout(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Disconnected)
    }
}

/// A `DevupAuth` double that overrides `direct_path_snapshot` and
/// `configure_client_credentials`, unlike the plain `AuthProbe` above
/// which relies on the trait's default implementations. Used to verify
/// the server plumbing actually calls through to these methods and
/// surfaces their result verbatim, rather than the default fallback.
struct RichAuthProbe {
    status: AuthStatus,
    snapshot: DirectPathSnapshot,
    configured: Mutex<Option<(String, Option<String>)>>,
}

#[async_trait]
impl DevupAuth for RichAuthProbe {
    async fn status(&self) -> Result<AuthStatus, DevupError> {
        Ok(self.status)
    }

    async fn login(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Connected)
    }

    async fn logout(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Disconnected)
    }

    async fn direct_path_snapshot(&self) -> Result<DirectPathSnapshot, DevupError> {
        Ok(self.snapshot.clone())
    }

    async fn configure_client_credentials(
        &self,
        client_id: String,
        client_secret: Option<String>,
    ) -> Result<(), DevupError> {
        *self.configured.lock().await = Some((client_id, client_secret));
        Ok(())
    }
}

#[derive(Default)]
struct UnavailableUpstream {
    calls: AtomicUsize,
}

#[async_trait]
impl FigmaUpstream for UnavailableUpstream {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["get_metadata".to_owned(), "use_figma".to_owned()])
    }

    async fn call_read_tool(&self, _call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(DevupError::new(
            ErrorCode::DevupFigmaDirectUnavailable,
            "synthetic direct failure",
            false,
        ))
    }
}

async fn call_named_tool(
    auth: Arc<dyn DevupAuth>,
    upstream: Arc<dyn FigmaUpstream>,
    tool: &str,
    arguments: Value,
) -> anyhow::Result<CallToolResult> {
    let server = DevupServer::new(Services::new(auth, upstream));
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let arguments: Map<String, Value> = arguments.as_object().cloned().unwrap();
    let result = client
        .call_tool(CallToolRequestParams::new(tool.to_owned()).with_arguments(arguments))
        .await?;
    client.cancel().await?;
    task.await??;
    Ok(result)
}

#[tokio::test]
async fn doctor_action_reports_measured_paths_and_client_setup_data() -> anyhow::Result<()> {
    let result = call_named_tool(
        Arc::new(AuthProbe {
            status: AuthStatus::Disconnected,
        }),
        Arc::new(UnavailableUpstream::default()),
        "devup_figma_auth",
        json!({ "action": "doctor" }),
    )
    .await?;
    let output = result.structured_content.unwrap();

    assert_eq!(output["status"], "disconnected");
    assert_eq!(output["paths"]["direct"]["available"], false);
    assert!(output["paths"]["direct"]["reason"].is_string());
    assert_eq!(output["paths"]["hostHandoff"]["expectedTool"], "use_figma");

    let client_setup = &output["clientSetup"];
    assert!(client_setup["constraints"]["clientNameAllowlist"].is_string());
    assert!(client_setup["constraints"]["redirectUri"].is_string());
    assert!(client_setup["constraints"]["callbackPortCaution"].is_string());
    assert!(client_setup["constraints"]["personalAccessToken"].is_string());
    // Codex is the primary, self-contained install path; the other hosts
    // remain reachable but demoted under `otherHosts`.
    assert_eq!(client_setup["codex"]["primary"], true);
    assert!(
        client_setup["codex"]["installDevupMcp"]["toml"]
            .as_str()
            .unwrap()
            .contains("[mcp_servers.devup-mcp]")
    );
    assert!(
        client_setup["codex"]["officialFigmaMcp"]
            .as_str()
            .unwrap()
            .contains("figma")
    );
    assert!(client_setup["otherHosts"]["opencode"]["example"]["mcp"]["figma"]["oauth"].is_object());
    assert!(
        client_setup["otherHosts"]["claudeCode"]
            .as_str()
            .unwrap()
            .contains("figma")
    );

    // No actual credential material, ever. `clientSetup` legitimately
    // documents *where* clientId/clientSecret/PAT go (field names and a
    // `figd_...` prefix example), as reference text, not real secrets.
    let raw = output.to_string();
    assert!(!raw.contains("accessToken"));
    assert!(!raw.contains("refreshToken"));
    Ok(())
}

#[tokio::test]
async fn doctor_action_reflects_connected_status_without_changing_the_status_action_shape()
-> anyhow::Result<()> {
    let doctor = call_named_tool(
        Arc::new(AuthProbe {
            status: AuthStatus::Connected,
        }),
        Arc::new(UnavailableUpstream::default()),
        "devup_figma_auth",
        json!({ "action": "doctor" }),
    )
    .await?
    .structured_content
    .unwrap();
    assert_eq!(doctor["status"], "connected");
    assert_eq!(doctor["paths"]["direct"]["available"], true);

    // The pre-existing `status` action must keep returning exactly
    // `{"status": ...}` so existing callers stay compatible.
    let status = call_named_tool(
        Arc::new(AuthProbe {
            status: AuthStatus::Connected,
        }),
        Arc::new(UnavailableUpstream::default()),
        "devup_figma_auth",
        json!({ "action": "status" }),
    )
    .await?
    .structured_content
    .unwrap();
    assert_eq!(status, json!({ "status": "connected" }));
    Ok(())
}

#[tokio::test]
async fn needs_figma_always_carries_an_actionable_host_requirement() -> anyhow::Result<()> {
    let result = call_named_tool(
        Arc::new(AuthProbe {
            status: AuthStatus::Disconnected,
        }),
        Arc::new(UnavailableUpstream::default()),
        "devup_figma_to_ui",
        json!({
            "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-2",
            "sourcePolicy": "auto"
        }),
    )
    .await?;
    let output = result.structured_content.unwrap();

    assert_eq!(output["status"], "needs_figma");
    let host_requirement = &output["hostRequirement"];
    assert!(
        host_requirement["reason"]
            .as_str()
            .unwrap()
            .contains("Figma")
    );
    assert!(host_requirement["steps"].as_array().unwrap().len() >= 4);
    assert_eq!(
        host_requirement["ifUnavailable"]["action"],
        "stop-and-report"
    );
    assert!(
        host_requirement["ifUnavailable"]["message"]
            .as_str()
            .unwrap()
            .contains("guessing")
    );
    assert!(
        host_requirement["ifUnavailable"]["setupHint"]
            .as_str()
            .unwrap()
            .contains("doctor")
    );
    Ok(())
}

/// The core deliverable of the handoff-completion fix: every `needs_figma`
/// step must carry `hostRequirement.resultContract` (so the agent submits
/// the right shape from the start) and `hostRequirement.outputExpectation`
/// (so it never falls back to hand-interpreting `use_figma`'s raw node
/// tree while waiting for devup-mcp's own TSX). See the real incident this
/// fixes in `crates/devup-mcp/src/server/handoff.rs`'s module docs.
#[tokio::test]
async fn needs_figma_always_carries_result_contract_and_output_expectation() -> anyhow::Result<()> {
    let result = call_named_tool(
        Arc::new(AuthProbe {
            status: AuthStatus::Disconnected,
        }),
        Arc::new(UnavailableUpstream::default()),
        "devup_figma_to_ui",
        json!({
            "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-2",
            "sourcePolicy": "auto"
        }),
    )
    .await?;
    let output = result.structured_content.unwrap();
    assert_eq!(output["status"], "needs_figma");
    let host_requirement = &output["hostRequirement"];

    let result_contract = &host_requirement["resultContract"];
    assert!(!result_contract["expects"].as_str().unwrap().is_empty());
    assert!(
        result_contract["ifHostFlattensToText"]
            .as_str()
            .unwrap()
            .contains("content")
    );
    assert!(
        result_contract["neverFabricate"]
            .as_str()
            .unwrap()
            .contains("structuredContent")
    );

    let output_expectation = &host_requirement["outputExpectation"];
    assert!(
        output_expectation["whatYouWillGet"]
            .as_str()
            .unwrap()
            .contains("devup-ui")
    );
    let do_not_hand_interpret = output_expectation["doNotHandInterpret"].as_str().unwrap();
    assert!(do_not_hand_interpret.contains("node tree"));
    assert!(do_not_hand_interpret.contains("devup-ui"));
    assert!(
        output_expectation["ifConversionFails"]
            .as_str()
            .unwrap()
            .contains("stop-and-report")
    );
    Ok(())
}

#[tokio::test]
async fn host_policy_needs_figma_also_carries_the_host_requirement() -> anyhow::Result<()> {
    let result = call_named_tool(
        Arc::new(AuthProbe {
            status: AuthStatus::Connected,
        }),
        Arc::new(UnavailableUpstream::default()),
        "devup_figma_to_ui",
        json!({
            "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-2",
            "sourcePolicy": "host"
        }),
    )
    .await?;
    let output = result.structured_content.unwrap();

    assert_eq!(output["status"], "needs_figma");
    assert_eq!(
        output["hostRequirement"]["ifUnavailable"]["action"],
        "stop-and-report"
    );
    // resultContract/outputExpectation must be present regardless of which
    // sourcePolicy triggered the handoff.
    assert!(
        output["hostRequirement"]["resultContract"]["expects"]
            .as_str()
            .is_some()
    );
    assert!(
        output["hostRequirement"]["outputExpectation"]["doNotHandInterpret"]
            .as_str()
            .is_some()
    );
    Ok(())
}

/// A `DevupAuth` double that does not override `direct_path_snapshot`
/// (like `AuthProbe`) must still produce a shape-complete `doctor`
/// response via the trait's default implementation, so pre-existing
/// `DevupAuth` implementors outside this crate keep compiling *and*
/// keep working after this task's `credentialSource`/`tokenState`/
/// `callbackPort` additions.
#[tokio::test]
async fn doctor_falls_back_to_default_direct_path_snapshot_for_plain_auth_doubles()
-> anyhow::Result<()> {
    let output = call_named_tool(
        Arc::new(AuthProbe {
            status: AuthStatus::Disconnected,
        }),
        Arc::new(UnavailableUpstream::default()),
        "devup_figma_auth",
        json!({ "action": "doctor" }),
    )
    .await?
    .structured_content
    .unwrap();

    assert_eq!(output["paths"]["direct"]["credentialSource"], "none");
    assert_eq!(output["paths"]["direct"]["tokenState"], "absent");
    assert!(output["paths"]["direct"]["callbackPort"]["port"].is_null());
    assert!(output["paths"]["direct"]["callbackPort"]["free"].is_null());

    let connected = call_named_tool(
        Arc::new(AuthProbe {
            status: AuthStatus::Connected,
        }),
        Arc::new(UnavailableUpstream::default()),
        "devup_figma_auth",
        json!({ "action": "doctor" }),
    )
    .await?
    .structured_content
    .unwrap();
    assert_eq!(connected["paths"]["direct"]["tokenState"], "valid");
    Ok(())
}

/// The core deliverable of this task's `doctor` update: `paths.direct`
/// must reflect the real, measured `credentialSource`/`tokenState`/
/// `callbackPort` from a `DevupAuth` implementation that actually tracks
/// them (here `RichAuthProbe`, standing in for the real `OAuthManager`).
#[tokio::test]
async fn doctor_reports_measured_credential_source_token_state_and_callback_port()
-> anyhow::Result<()> {
    let auth = RichAuthProbe {
        status: AuthStatus::Disconnected,
        snapshot: DirectPathSnapshot {
            credential_source: ClientCredentialSource::CliArg,
            token_state: TokenState::Expired,
            callback_port: Some(19876),
            callback_port_free: Some(false),
            client_name: DEFAULT_CLIENT_NAME.to_owned(),
        },
        configured: Mutex::new(None),
    };
    let output = call_named_tool(
        Arc::new(auth),
        Arc::new(UnavailableUpstream::default()),
        "devup_figma_auth",
        json!({ "action": "doctor" }),
    )
    .await?
    .structured_content
    .unwrap();

    assert_eq!(output["paths"]["direct"]["credentialSource"], "cli-arg");
    assert_eq!(output["paths"]["direct"]["tokenState"], "expired");
    assert_eq!(output["paths"]["direct"]["callbackPort"]["port"], 19876);
    assert_eq!(output["paths"]["direct"]["callbackPort"]["free"], false);
    Ok(())
}

/// `devup_figma_auth {"action":"configure"}` must persist the given
/// `clientId`/`clientSecret` via the auth backend, respond with only
/// `{"status":"configured"}` (never echoing the secret back), and reject
/// a missing `clientId` before ever calling the auth backend.
#[tokio::test]
async fn configure_action_persists_credentials_and_never_echoes_the_secret() -> anyhow::Result<()> {
    let auth = Arc::new(RichAuthProbe {
        status: AuthStatus::Disconnected,
        snapshot: DirectPathSnapshot {
            credential_source: ClientCredentialSource::None,
            token_state: TokenState::Absent,
            callback_port: None,
            callback_port_free: None,
            client_name: DEFAULT_CLIENT_NAME.to_owned(),
        },
        configured: Mutex::new(None),
    });
    let result = call_named_tool(
        auth.clone(),
        Arc::new(UnavailableUpstream::default()),
        "devup_figma_auth",
        json!({
            "action": "configure",
            "clientId": "preregistered-client",
            "clientSecret": "preregistered-secret"
        }),
    )
    .await?;
    let output = result.structured_content.unwrap();
    assert_eq!(output, json!({ "status": "configured" }));
    let raw = output.to_string();
    assert!(!raw.contains("preregistered-secret"));

    let captured = auth.configured.lock().await.clone();
    assert_eq!(
        captured,
        Some((
            "preregistered-client".to_owned(),
            Some("preregistered-secret".to_owned())
        ))
    );
    Ok(())
}

#[tokio::test]
async fn configure_action_without_client_id_is_rejected() -> anyhow::Result<()> {
    let error = call_named_tool(
        Arc::new(AuthProbe {
            status: AuthStatus::Disconnected,
        }),
        Arc::new(UnavailableUpstream::default()),
        "devup_figma_auth",
        json!({ "action": "configure" }),
    )
    .await
    .expect_err("configure without clientId must fail");
    assert!(error.to_string().contains("clientId"));
    Ok(())
}

/// `DevupAuth` implementations that do not support persisting a client
/// credential (the trait's default `configure_client_credentials`) must
/// surface that as an explicit tool error, not silently succeed.
#[tokio::test]
async fn configure_action_fails_for_auth_backends_that_do_not_support_it() -> anyhow::Result<()> {
    let error = call_named_tool(
        Arc::new(AuthProbe {
            status: AuthStatus::Disconnected,
        }),
        Arc::new(UnavailableUpstream::default()),
        "devup_figma_auth",
        json!({ "action": "configure", "clientId": "preregistered-client" }),
    )
    .await
    .expect_err("plain AuthProbe does not support configure");
    assert!(!error.to_string().is_empty());
    Ok(())
}

/// The Figma desktop app's local Dev Mode MCP serves six read tools and
/// `use_figma` is not among them, so every collection devup-mcp performs —
/// snapshot, explore, section index, theme — has no tool there to run. Its
/// tools also address whatever the desktop app currently has open rather than
/// a file key. It was reported as a third connection path and described as
/// usable without OAuth, and an agent that believed it spent its turn finding
/// out otherwise. Nothing devup-mcp says should name it.
#[tokio::test]
async fn nothing_offers_the_local_dev_mode_server_as_a_path() -> anyhow::Result<()> {
    let doctor = call_named_tool(
        Arc::new(AuthProbe {
            status: AuthStatus::Disconnected,
        }),
        Arc::new(UnavailableUpstream::default()),
        "devup_figma_auth",
        json!({ "action": "doctor" }),
    )
    .await?
    .structured_content
    .unwrap();
    let handoff = call_named_tool(
        Arc::new(AuthProbe {
            status: AuthStatus::Disconnected,
        }),
        Arc::new(UnavailableUpstream::default()),
        "devup_figma_export",
        json!({
            "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-2",
            "sourcePolicy": "host"
        }),
    )
    .await?
    .structured_content
    .unwrap();

    for (label, value) in [("doctor", &doctor), ("export handoff", &handoff)] {
        let rendered = serde_json::to_string(value)?;
        for forbidden in ["localDevMode", "3845", "Dev Mode"] {
            assert!(
                !rendered.contains(forbidden),
                "{label} still names the local Dev Mode server via {forbidden:?}"
            );
        }
    }
    Ok(())
}
