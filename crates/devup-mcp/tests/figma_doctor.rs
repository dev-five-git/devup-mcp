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
    AuthStatus, DevupError, ErrorCode, FigmaUpstream, ReadToolCall, UpstreamResult,
};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, CallToolResult},
};
use serde_json::{Map, Value, json};

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
    assert_eq!(
        output["paths"]["localDevMode"]["endpoint"],
        "http://127.0.0.1:3845/mcp"
    );
    assert!(output["paths"]["localDevMode"]["reachable"].is_boolean());
    assert_eq!(output["paths"]["hostHandoff"]["expectedTool"], "use_figma");

    let client_setup = &output["clientSetup"];
    assert!(client_setup["constraints"]["clientNameAllowlist"].is_string());
    assert!(client_setup["constraints"]["redirectUri"].is_string());
    assert!(client_setup["constraints"]["callbackPortCaution"].is_string());
    assert!(client_setup["constraints"]["personalAccessToken"].is_string());
    assert!(client_setup["opencode"]["example"]["mcp"]["figma"]["oauth"].is_object());
    assert!(
        client_setup["claudeCode"]
            .as_str()
            .unwrap()
            .contains("figma")
    );
    assert!(client_setup["codex"].as_str().unwrap().contains("figma"));
    assert_eq!(
        client_setup["localDevMode"]["endpoint"],
        "http://127.0.0.1:3845/mcp"
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
            .contains("추측")
    );
    assert!(
        host_requirement["ifUnavailable"]["setupHint"]
            .as_str()
            .unwrap()
            .contains("doctor")
    );
    assert!(host_requirement["localDevMode"]["reachable"].is_boolean());
    assert_eq!(
        host_requirement["localDevMode"]["endpoint"],
        "http://127.0.0.1:3845/mcp"
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
    assert!(do_not_hand_interpret.contains("노드 트리"));
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
