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

#[derive(Debug)]
struct TruncatedSectionUpstream;

#[async_trait]
impl FigmaUpstream for TruncatedSectionUpstream {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".into()])
    }
    async fn call_read_tool(&self, _call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        Ok(UpstreamResult {
            raw: json!({"content":[{"type":"text","text":"{\"fileKey\":\"SECRET\",\"nodes\":[// truncated to 20kb"}]}),
        })
    }
}

#[tokio::test]
async fn r11_tool_failure_has_identical_content_and_structured_error() -> anyhow::Result<()> {
    let server = DevupServer::new(Services::new(
        Arc::new(ConnectedAuth),
        Arc::new(TruncatedSectionUpstream),
    ));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let response = client.call_tool(CallToolRequestParams::new("devup_figma_export").with_arguments(
        json!({"url":"https://www.figma.com/design/FileKey123/Fixture?node-id=10-1","frameIds":["10:2"],"outputs":["tsx"]}).as_object().unwrap().clone()
    )).await.expect("tool execution failures are structured tool results");
    assert_eq!(response.is_error, Some(true));
    let wire = serde_json::to_value(&response)?;
    let value = response.structured_content.expect("structured error");
    assert_eq!(
        serde_json::from_str::<Value>(wire["content"][0]["text"].as_str().unwrap())?,
        value
    );
    assert_eq!(value["error"]["code"], "DEVUP_SNAPSHOT_UNSUPPORTED");
    assert_eq!(value["error"]["retryable"], false);
    assert_eq!(value["error"]["details"]["category"], "truncated-text");
    assert!(value["server"].is_object());
    assert!(!value.to_string().contains("SECRET"));
    client.cancel().await?;
    task.abort();
    Ok(())
}

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

/// The same refusal with no `isError` on it at all.
///
/// One arrived this way during a real capture: unflagged, so it passed
/// straight through to the collector, which searched it for the variable batch
/// it did not contain and reported "variable/style batch not found in the
/// Figma MCP response" — twenty-four minutes in, naming the parser that
/// happened to be next rather than the refusal that was there all along.
#[derive(Debug)]
struct UnflaggedRefusal;

#[async_trait]
impl FigmaUpstream for UnflaggedRefusal {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".to_owned()])
    }
    async fn call_read_tool(&self, _call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        Ok(UpstreamResult {
            raw: json!({
                "content": [{"type": "text", "text": RATE_LIMIT_TEXT}]
            }),
        })
    }
}

/// A response that is not a refusal but says the words, at the length a real
/// payload has. A design may name a layer anything, so recognising a refusal
/// by its text has to stop short of failing a collection that worked.
#[derive(Debug)]
struct PayloadMentioningTheLimit;

#[async_trait]
impl FigmaUpstream for PayloadMentioningTheLimit {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".to_owned()])
    }
    async fn call_read_tool(&self, _call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        let padding = "x".repeat(4000);
        Ok(UpstreamResult {
            raw: json!({
                "content": [{
                    "type": "text",
                    "text": format!("{{\"nodes\":{{\"1:2\":{{\"name\":\"rate limit banner\"}}}},\"pad\":\"{padding}\"}}")
                }]
            }),
        })
    }
}

/// Same refusal, but with the wait Figma's REST API states in `Retry-After`.
/// The MCP relay does not forward it today; this pins that it is used the
/// moment it appears, rather than the caller being told to guess.
#[derive(Debug)]
struct RateLimitedWithRetryAfter;

#[async_trait]
impl FigmaUpstream for RateLimitedWithRetryAfter {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".to_owned()])
    }
    async fn call_read_tool(&self, _call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        Ok(UpstreamResult {
            raw: json!({
                "content": [{"type": "text", "text": RATE_LIMIT_TEXT}],
                "isError": true,
                "headers": {"Retry-After": 42}
            }),
        })
    }
}

/// Runs an export against `upstream` and returns what the caller is told.
async fn reported_failure(upstream: Arc<dyn FigmaUpstream>) -> anyhow::Result<String> {
    let server = DevupServer::new(Services::new(Arc::new(ConnectedAuth), upstream));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let arguments: Map<String, Value> = json!({
        "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=10-1",
        "outputs": ["tsx"]
    })
    .as_object()
    .cloned()
    .expect("arguments object");

    let reported = client
        .call_tool(CallToolRequestParams::new("devup_figma_export").with_arguments(arguments))
        .await
        .expect("tool error response");
    assert_eq!(reported.is_error, Some(true));
    let reported = reported
        .structured_content
        .expect("structured error")
        .to_string();

    client.cancel().await?;
    task.abort();
    Ok(reported)
}

/// A refusal is recognised by what it says, not only by how it is flagged.
#[tokio::test]
async fn an_unflagged_refusal_is_still_read_as_one() -> anyhow::Result<()> {
    let reported = reported_failure(Arc::new(UnflaggedRefusal)).await?;

    assert!(
        reported.contains("DEVUP_FIGMA_RATE_LIMITED"),
        "an unflagged refusal is still a refusal: {reported}"
    );
    assert!(
        !reported.contains("not found in the Figma MCP response"),
        "it must not be blamed on whichever parser was next: {reported}"
    );
    Ok(())
}

/// And the recognition stops at the length a refusal can be, so a payload that
/// merely contains the words is still collected rather than refused.
#[tokio::test]
async fn a_payload_that_mentions_the_limit_is_not_mistaken_for_one() -> anyhow::Result<()> {
    let reported = reported_failure(Arc::new(PayloadMentioningTheLimit)).await?;

    assert!(
        !reported.contains("DEVUP_FIGMA_RATE_LIMITED"),
        "a design may name a layer anything; that is not a refusal: {reported}"
    );
    Ok(())
}

#[tokio::test]
async fn a_stated_retry_after_is_reported_instead_of_a_guess() -> anyhow::Result<()> {
    let server = DevupServer::new(Services::new(
        Arc::new(ConnectedAuth),
        Arc::new(RateLimitedWithRetryAfter),
    ));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let arguments: Map<String, Value> = json!({
        "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=10-1",
        "outputs": ["tsx"]
    })
    .as_object()
    .cloned()
    .expect("arguments object");

    let reported = client
        .call_tool(CallToolRequestParams::new("devup_figma_export").with_arguments(arguments))
        .await
        .expect("tool error response");
    assert_eq!(reported.is_error, Some(true));
    let reported = reported
        .structured_content
        .expect("structured error")
        .to_string();

    assert!(
        reported.contains("\"retryAfterSeconds\":42"),
        "the stated wait must be surfaced: {reported}"
    );
    assert!(
        !reported.contains("Not stated"),
        "a stated wait must not also be reported as unstated: {reported}"
    );

    client.cancel().await?;
    task.abort();
    Ok(())
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
        "outputs": ["tsx"]
    })
    .as_object()
    .cloned()
    .expect("arguments object");

    let error = client
        .call_tool(CallToolRequestParams::new("devup_figma_export").with_arguments(arguments))
        .await
        .expect("tool error response");
    assert_eq!(error.is_error, Some(true));
    let reported = error
        .structured_content
        .expect("structured error")
        .to_string();

    assert!(
        reported.contains("tool call limit"),
        "the upstream reason must survive: {reported}"
    );
    assert!(
        !reported.contains("not found in the Figma MCP response"),
        "the refusal must not be reported as missing data: {reported}"
    );
    // A quota refusal clears on its own, so reporting it as permanent would
    // tell the caller to give up on something that fixes itself.
    assert!(
        reported.contains("DEVUP_FIGMA_RATE_LIMITED"),
        "a quota refusal must be classified as one: {reported}"
    );
    assert!(
        reported.contains("\"retryable\":true"),
        "a quota refusal must be retryable: {reported}"
    );
    // Figma meters with a leaky bucket, so promising a reset would send the
    // caller waiting for a rollover that never arrives.
    assert!(
        reported.contains("leaky bucket"),
        "recovery must be described as gradual: {reported}"
    );
    // This relay forwards no Retry-After, so the response must admit that
    // rather than pick a ceiling on the caller's behalf.
    assert!(
        reported.contains("Not stated"),
        "an unstated ceiling must be reported as unstated: {reported}"
    );

    client.cancel().await?;
    task.abort();
    Ok(())
}
