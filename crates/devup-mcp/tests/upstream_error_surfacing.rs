//! An upstream refusal must be reported as itself.
//!
//! MCP delivers a refusal as a *successful* tool call whose result carries
//! `isError`. Handing that to the collector made it search the response for
//! data that was never in it and then blame the parser — "metadata not found
//! in the Figma MCP response", or the equivalent for snapshot data, variable
//! batches or asset descriptors, depending only on which step happened to
//! receive it. The reason was in the response all along.

use std::sync::Arc;

use async_trait::async_trait;
use devup_mcp::server::{DevupAuth, DevupServer, Services};
use devup_mcp_figma::{AuthStatus, DevupError, FigmaUpstream, ReadToolCall, UpstreamResult};
use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Map, Value, json};

/// Verbatim shape of a real Figma rate-limit response.
const RATE_LIMIT_TEXT: &str = "You've reached the Figma MCP tool call limit for your Full seat on the Professional plan. Upgrade your seat or plan for more tool calls.";

#[derive(Debug)]
struct ConnectedAuth;

#[async_trait]
impl DevupAuth for ConnectedAuth {
    async fn status(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Connected)
    }
    async fn login(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Connected)
    }
    async fn logout(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Disconnected)
    }
}

#[derive(Debug)]
struct RateLimitedUpstream;

#[async_trait]
impl FigmaUpstream for RateLimitedUpstream {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".to_owned()])
    }
    async fn call_read_tool(&self, _call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        Ok(UpstreamResult {
            raw: json!({
                "content": [
                    {"type": "text", "text": RATE_LIMIT_TEXT},
                    {"type": "resource_link", "uri": "file://figma/docs/rate-limits-access.md"}
                ],
                "isError": true
            }),
        })
    }
}

#[tokio::test]
async fn a_rate_limited_upstream_reports_its_own_reason_not_a_parse_failure() -> anyhow::Result<()>
{
    let server = DevupServer::new(Services::new(
        Arc::new(ConnectedAuth),
        Arc::new(RateLimitedUpstream),
    ));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let arguments: Map<String, Value> = json!({
        "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=10-1",
        "outputs": ["tsx"],
        "sourcePolicy": "direct"
    })
    .as_object()
    .cloned()
    .expect("arguments object");

    let error = client
        .call_tool(CallToolRequestParams::new("devup_figma_export").with_arguments(arguments))
        .await
        .expect_err("a refused collection must fail");
    let reported = error.to_string();

    assert!(
        reported.contains("tool call limit"),
        "the upstream reason must survive: {reported}"
    );
    assert!(
        !reported.contains("not found in the Figma MCP response"),
        "the refusal must not be reported as missing data: {reported}"
    );

    client.cancel().await?;
    task.abort();
    Ok(())
}
