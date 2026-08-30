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
    logins: AtomicUsize,
}

#[async_trait]
impl DevupAuth for AuthProbe {
    async fn status(&self) -> Result<AuthStatus, DevupError> {
        Ok(self.status)
    }

    async fn login(&self) -> Result<AuthStatus, DevupError> {
        self.logins.fetch_add(1, Ordering::SeqCst);
        Ok(AuthStatus::Connected)
    }

    async fn logout(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Disconnected)
    }
}

struct UpstreamProbe {
    calls: AtomicUsize,
    error_code: ErrorCode,
}

impl UpstreamProbe {
    fn unavailable() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            error_code: ErrorCode::DevupFigmaDirectUnavailable,
        }
    }
}

#[async_trait]
impl FigmaUpstream for UpstreamProbe {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["get_metadata".to_owned(), "use_figma".to_owned()])
    }

    async fn call_read_tool(&self, _call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(DevupError::new(
            self.error_code,
            "synthetic direct failure",
            false,
        ))
    }
}

#[derive(Default)]
struct FixtureUpstream {
    calls: AtomicUsize,
}

#[async_trait]
impl FigmaUpstream for FixtureUpstream {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["get_metadata".to_owned(), "use_figma".to_owned()])
    }

    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let raw = match call.tool_name() {
            "get_metadata" => metadata_result(),
            "use_figma" => snapshot_result(),
            _ => unreachable!(),
        };
        Ok(UpstreamResult { raw })
    }
}

async fn call_tool(
    auth: Arc<dyn DevupAuth>,
    upstream: Arc<dyn FigmaUpstream>,
    arguments: Value,
) -> anyhow::Result<CallToolResult> {
    call_named_tool(auth, upstream, "devup_figma_to_ui", arguments).await
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
async fn search_collects_the_file_and_returns_replayable_node_urls() -> anyhow::Result<()> {
    let result = call_named_tool(
        Arc::new(AuthProbe {
            status: AuthStatus::Connected,
            logins: AtomicUsize::new(0),
        }),
        Arc::new(FixtureUpstream::default()),
        "devup_figma_search",
        json!({
            "url": "https://www.figma.com/design/FileKey123/Fixture",
            "query": "syntheticframe",
            "sourcePolicy": "direct"
        }),
    )
    .await?;
    let output = result.structured_content.unwrap();
    assert_eq!(output["status"], "complete");
    assert_eq!(output["count"], 1);
    assert_eq!(output["matches"][0]["nodeId"], "1:2");
    assert_eq!(output["matches"][0]["matchKind"], "normalized-exact");
    assert_eq!(
        output["matches"][0]["canonicalUrl"],
        "https://www.figma.com/design/FileKey123/devup?node-id=1-2"
    );
    Ok(())
}

#[tokio::test]
async fn ui_output_path_writes_the_generated_artifact_only_when_requested() -> anyhow::Result<()> {
    let path = std::env::temp_dir().join(format!(
        "devup-mcp-output-{}-{}.tsx",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let result = call_tool(
        Arc::new(AuthProbe {
            status: AuthStatus::Connected,
            logins: AtomicUsize::new(0),
        }),
        Arc::new(FixtureUpstream::default()),
        json!({
            "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-2",
            "sourcePolicy": "direct",
            "outputPath": path
        }),
    )
    .await?;
    let output = result.structured_content.unwrap();
    let written = std::fs::read_to_string(&path)?;
    assert_eq!(written, output["tsx"].as_str().unwrap());
    assert!(output["outputPath"].as_str().is_some());
    std::fs::remove_file(path)?;
    Ok(())
}

fn input(policy: &str) -> Value {
    json!({
        "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-2",
        "sourcePolicy": policy
    })
}

fn metadata_result() -> Value {
    json!({
        "structuredContent": {
            "devupMetadata": {
                "fileKey": "FileKey123",
                "version": "v1",
                "rootId": "1:2",
                "nodes": [{
                    "id": "1:2", "type": "FRAME", "childrenIds": [], "descendantCount": 1
                }]
            }
        }
    })
}

fn snapshot_result() -> Value {
    json!({
        "fileKey": "FileKey123",
        "version": "v1",
        "rootIds": ["1:2"],
        "nodes": [{
            "id": "1:2", "type": "FRAME",
            "fields": {
                "name": "Synthetic Frame", "childrenIds": [],
                "layoutMode": "VERTICAL", "width": 320, "height": 240
            },
            "extra": {}, "fieldErrors": {}
        }]
    })
}

#[tokio::test]
async fn auto_disconnected_returns_handoff_without_starting_oauth() -> anyhow::Result<()> {
    let auth = Arc::new(AuthProbe {
        status: AuthStatus::Disconnected,
        logins: AtomicUsize::new(0),
    });
    let upstream = Arc::new(UpstreamProbe::unavailable());
    let result = call_tool(auth.clone(), upstream.clone(), input("auto")).await?;
    let output = result.structured_content.unwrap();

    assert_eq!(output["status"], "needs_figma");
    assert_eq!(output["resumeTool"], "devup_figma_continue");
    assert_eq!(output["calls"][0]["tool"], "get_metadata");
    assert!(output["expiresAt"].as_str().unwrap().contains('T'));
    assert!(output["expiresAt"].as_str().unwrap().ends_with('Z'));
    assert_eq!(auth.logins.load(Ordering::SeqCst), 0);
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn host_policy_never_calls_direct_auth_or_upstream() -> anyhow::Result<()> {
    let auth = Arc::new(AuthProbe {
        status: AuthStatus::Connected,
        logins: AtomicUsize::new(0),
    });
    let upstream = Arc::new(UpstreamProbe::unavailable());
    let result = call_tool(auth.clone(), upstream.clone(), input("host")).await?;
    let output = result.structured_content.unwrap();

    assert_eq!(output["status"], "needs_figma");
    assert_eq!(auth.logins.load(Ordering::SeqCst), 0);
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn direct_disconnected_never_starts_oauth() -> anyhow::Result<()> {
    let auth = Arc::new(AuthProbe {
        status: AuthStatus::Disconnected,
        logins: AtomicUsize::new(0),
    });
    let upstream = Arc::new(UpstreamProbe::unavailable());
    let result = call_tool(auth.clone(), upstream.clone(), input("direct")).await;

    assert!(result.is_err());
    assert_eq!(auth.logins.load(Ordering::SeqCst), 0);
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn connected_auto_completes_through_the_direct_collector() -> anyhow::Result<()> {
    let auth = Arc::new(AuthProbe {
        status: AuthStatus::Connected,
        logins: AtomicUsize::new(0),
    });
    let upstream = Arc::new(FixtureUpstream::default());
    let result = call_tool(auth.clone(), upstream.clone(), input("auto")).await?;
    let output = result.structured_content.unwrap();

    assert_eq!(output["status"], "complete");
    assert_eq!(output["source"]["kind"], "direct");
    assert!(output["tsx"].as_str().unwrap().contains("SyntheticFrame"));
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 2);
    assert_eq!(auth.logins.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn auto_falls_back_for_capability_failure_but_not_rate_limit() -> anyhow::Result<()> {
    let auth = Arc::new(AuthProbe {
        status: AuthStatus::Connected,
        logins: AtomicUsize::new(0),
    });
    let unavailable = Arc::new(UpstreamProbe::unavailable());
    let fallback = call_tool(auth.clone(), unavailable, input("auto")).await?;
    assert_eq!(
        fallback.structured_content.unwrap()["status"],
        "needs_figma"
    );

    let rate_limited = Arc::new(UpstreamProbe {
        calls: AtomicUsize::new(0),
        error_code: ErrorCode::DevupFigmaRateLimited,
    });
    let rejected = call_tool(auth, rate_limited.clone(), input("auto")).await;
    assert!(rejected.is_err());
    assert_eq!(rate_limited.calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn public_continuation_finishes_a_multi_call_host_collection() -> anyhow::Result<()> {
    let auth = Arc::new(AuthProbe {
        status: AuthStatus::Disconnected,
        logins: AtomicUsize::new(0),
    });
    let upstream = Arc::new(UpstreamProbe::unavailable());
    let server = DevupServer::new(Services::new(auth, upstream));
    let (server_transport, client_transport) = tokio::io::duplex(128 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let start = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_to_ui")
                .with_arguments(input("host").as_object().cloned().unwrap()),
        )
        .await?
        .structured_content
        .unwrap();
    let session_id = start["sessionId"].as_str().unwrap();
    let metadata_call = start["calls"][0]["callId"].as_str().unwrap();
    let after_metadata = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_continue").with_arguments(
                json!({
                    "sessionId": session_id,
                    "callId": metadata_call,
                    "result": metadata_result()
                })
                .as_object()
                .cloned()
                .unwrap(),
            ),
        )
        .await?
        .structured_content
        .unwrap();
    assert_eq!(after_metadata["status"], "needs_figma");
    assert_eq!(after_metadata["calls"][0]["tool"], "use_figma");

    let snapshot_call = after_metadata["calls"][0]["callId"].as_str().unwrap();
    let complete = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_continue").with_arguments(
                json!({
                    "sessionId": session_id,
                    "callId": snapshot_call,
                    "result": snapshot_result()
                })
                .as_object()
                .cloned()
                .unwrap(),
            ),
        )
        .await?
        .structured_content
        .unwrap();
    assert_eq!(complete["status"], "complete");
    assert_eq!(complete["source"]["kind"], "host");
    assert!(complete["tsx"].as_str().unwrap().contains("SyntheticFrame"));

    client.cancel().await?;
    task.await??;
    Ok(())
}

#[tokio::test]
async fn direct_and_host_collection_produce_identical_artifacts() -> anyhow::Result<()> {
    let auth = Arc::new(AuthProbe {
        status: AuthStatus::Connected,
        logins: AtomicUsize::new(0),
    });
    let upstream = Arc::new(FixtureUpstream::default());
    let server = DevupServer::new(Services::new(auth, upstream));
    let (server_transport, client_transport) = tokio::io::duplex(128 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let direct = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_to_ui")
                .with_arguments(input("direct").as_object().cloned().unwrap()),
        )
        .await?
        .structured_content
        .unwrap();
    let start = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_to_ui")
                .with_arguments(input("host").as_object().cloned().unwrap()),
        )
        .await?
        .structured_content
        .unwrap();
    let session_id = start["sessionId"].as_str().unwrap();
    let metadata_call = start["calls"][0]["callId"].as_str().unwrap();
    let after_metadata = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_continue").with_arguments(
                json!({
                    "sessionId": session_id,
                    "callId": metadata_call,
                    "result": metadata_result()
                })
                .as_object()
                .cloned()
                .unwrap(),
            ),
        )
        .await?
        .structured_content
        .unwrap();
    let snapshot_call = after_metadata["calls"][0]["callId"].as_str().unwrap();
    let host = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_continue").with_arguments(
                json!({
                    "sessionId": session_id,
                    "callId": snapshot_call,
                    "result": snapshot_result()
                })
                .as_object()
                .cloned()
                .unwrap(),
            ),
        )
        .await?
        .structured_content
        .unwrap();

    for field in ["tsx", "imports", "usedTokens", "diagnostics", "snapshot"] {
        assert_eq!(direct[field], host[field], "source changed {field}");
    }
    assert_eq!(direct["source"]["kind"], "direct");
    assert_eq!(host["source"]["kind"], "host");

    client.cancel().await?;
    task.await??;
    Ok(())
}
