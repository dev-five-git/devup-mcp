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

#[derive(Default)]
struct FastRejectingFixture {
    calls: AtomicUsize,
}

#[async_trait]
impl FigmaUpstream for FastRejectingFixture {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["get_metadata".to_owned(), "use_figma".to_owned()])
    }

    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        let index = self.calls.fetch_add(1, Ordering::SeqCst);
        if index == 0 {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaDirectUnavailable,
                "fast transport unavailable",
                false,
            ));
        }
        let raw = match call.tool_name() {
            "get_metadata" => metadata_result(),
            "use_figma" => snapshot_result(),
            _ => unreachable!(),
        };
        Ok(UpstreamResult { raw })
    }
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

async fn call_named_tool_with_roots(
    auth: Arc<dyn DevupAuth>,
    upstream: Arc<dyn FigmaUpstream>,
    tool: &str,
    arguments: Value,
    roots: Vec<std::path::PathBuf>,
) -> anyhow::Result<CallToolResult> {
    let server = DevupServer::with_output_roots(Services::new(auth, upstream), roots)?;
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
    let upstream = Arc::new(FixtureUpstream::default());
    let result = call_named_tool(
        Arc::new(AuthProbe {
            status: AuthStatus::Connected,
            logins: AtomicUsize::new(0),
        }),
        upstream.clone(),
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
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn ui_output_path_writes_the_generated_artifact_only_when_requested() -> anyhow::Result<()> {
    let root = std::env::temp_dir().join(format!(
        "devup-mcp-output-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir_all(&root)?;
    let path = root.join("generated.tsx");
    let result = call_named_tool_with_roots(
        Arc::new(AuthProbe {
            status: AuthStatus::Connected,
            logins: AtomicUsize::new(0),
        }),
        Arc::new(FixtureUpstream::default()),
        "devup_figma_to_ui",
        json!({
            "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-2",
            "sourcePolicy": "direct",
            "outputPath": path
        }),
        vec![root.clone()],
    )
    .await?;
    let output = result.structured_content.unwrap();
    let written = std::fs::read_to_string(&path)?;
    assert_eq!(written, output["tsx"].as_str().unwrap());
    assert!(output["outputPath"].as_str().is_some());
    std::fs::remove_file(path)?;
    std::fs::remove_dir(root)?;
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
                    "id": "1:2", "type": "FRAME", "name": "Synthetic Frame",
                    "childrenIds": [], "descendantCount": 1
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
                "layoutMode": "VERTICAL",
                "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                "width": 320, "height": 240
            },
            "extra": {}, "fieldErrors": {}
        }]
    })
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
    assert_eq!(output["rootLayout"], "standalone");
    assert!(output["tsx"].as_str().unwrap().contains("SyntheticFrame"));
    assert_eq!(output["collection"]["figmaToolCalls"], 3);
    assert_eq!(output["collection"]["transport"], "legacy-cursor");
    assert_eq!(output["collection"]["fallbackUsed"], true);
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 3);
    assert_eq!(auth.logins.load(Ordering::SeqCst), 0);

    // The unambiguous final-answer marker: without it, an agent that only
    // ever sees intermediate `needs_figma` steps has, in a real observed
    // failure, concluded the conversion was "probably done" and started
    // hand-interpreting the raw node tree instead of using this `tsx`.
    assert_eq!(output["deliverable"]["kind"], "devup-ui-tsx");
    assert_eq!(output["deliverable"]["isFinal"], true);
    assert!(!output["deliverable"]["note"].as_str().unwrap().is_empty());
    Ok(())
}

#[tokio::test]
async fn embedded_root_layout_omits_selected_frame_dimensions() -> anyhow::Result<()> {
    let result = call_tool(
        Arc::new(AuthProbe {
            status: AuthStatus::Connected,
            logins: AtomicUsize::new(0),
        }),
        Arc::new(FixtureUpstream::default()),
        json!({
            "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-2",
            "sourcePolicy": "direct",
            "rootLayout": "embedded"
        }),
    )
    .await?;
    let output = result.structured_content.unwrap();

    assert_eq!(output["rootLayout"], "embedded");
    let tsx = output["tsx"].as_str().unwrap();
    assert!(!tsx.contains("h=\"240px\""));
    assert!(!tsx.contains("w=\"320px\""));
    Ok(())
}

#[tokio::test]
async fn rejects_unknown_root_layout_before_collecting() -> anyhow::Result<()> {
    let upstream = Arc::new(FixtureUpstream::default());
    let result = call_tool(
        Arc::new(AuthProbe {
            status: AuthStatus::Connected,
            logins: AtomicUsize::new(0),
        }),
        upstream.clone(),
        json!({
            "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-2",
            "sourcePolicy": "direct",
            "rootLayout": "fluid"
        }),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn direct_fast_call_error_restarts_the_legacy_collector() -> anyhow::Result<()> {
    let upstream = Arc::new(FastRejectingFixture::default());
    let result = call_tool(
        Arc::new(AuthProbe {
            status: AuthStatus::Connected,
            logins: AtomicUsize::new(0),
        }),
        upstream.clone(),
        input("direct"),
    )
    .await?;
    let output = result.structured_content.unwrap();

    assert_eq!(output["status"], "complete");
    assert_eq!(output["collection"]["figmaToolCalls"], 3);
    assert_eq!(output["collection"]["fallbackUsed"], true);
    assert_eq!(
        output["collection"]["fallbackReason"],
        "DevupFigmaDirectUnavailable"
    );
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 3);
    Ok(())
}

/// Auto has one source now, so "auto" means direct and a refusal is reported
/// rather than handed anywhere else. What it must not do is log the caller in
/// on its own: a browser window they did not ask for, opened by a request for
/// code, long after the request that provoked it has scrolled away.
#[tokio::test]
async fn auto_asks_to_be_logged_in_rather_than_starting_oauth() -> anyhow::Result<()> {
    let auth = Arc::new(AuthProbe {
        status: AuthStatus::Disconnected,
        logins: AtomicUsize::new(0),
    });
    let upstream = Arc::new(UpstreamProbe::unavailable());
    let error = call_tool(auth.clone(), upstream.clone(), input("auto"))
        .await
        .expect_err("a disconnected direct path cannot collect");

    assert!(
        error.to_string().contains("devup_figma_auth login"),
        "the error should name the action that fixes it: {error}"
    );
    assert_eq!(auth.logins.load(Ordering::SeqCst), 0);
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

/// Every refusal now surfaces as itself. A capability that is missing says so
/// at once; a spent allowance is waited out three times first, because a
/// collection can cross a per-minute line partway through its own burst.
#[tokio::test(start_paused = true)]
async fn a_refusal_is_reported_as_itself() -> anyhow::Result<()> {
    let auth = Arc::new(AuthProbe {
        status: AuthStatus::Connected,
        logins: AtomicUsize::new(0),
    });

    let unavailable = Arc::new(UpstreamProbe::unavailable());
    assert!(
        call_tool(auth.clone(), unavailable.clone(), input("auto"))
            .await
            .is_err()
    );
    assert!(unavailable.calls.load(Ordering::SeqCst) >= 1);

    let rate_limited = Arc::new(UpstreamProbe {
        calls: AtomicUsize::new(0),
        error_code: ErrorCode::DevupFigmaRateLimited,
    });
    assert!(
        call_tool(auth, rate_limited.clone(), input("auto"))
            .await
            .is_err()
    );
    assert_eq!(rate_limited.calls.load(Ordering::SeqCst), 3);
    Ok(())
}
