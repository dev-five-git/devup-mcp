//! A collection is a burst: a Section spends five to seventeen calls back to
//! back and Figma meters by the minute, so a large enough target outruns its
//! own allowance partway through. That refusal used to end the collection and
//! return nothing, spending the allowance for no result at all.

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

/// Refuses the first `refusals` calls the way a spent allowance does, then
/// answers. Counts every attempt so the test can tell a retry from a give-up.
struct SpentAllowance {
    refusals: AtomicUsize,
    attempts: AtomicUsize,
    retry_after_seconds: Option<u64>,
}

#[async_trait]
impl FigmaUpstream for SpentAllowance {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["get_metadata".to_owned(), "use_figma".to_owned()])
    }

    async fn call_read_tool(&self, _call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        if self
            .refusals
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(1)
            })
            .is_ok()
        {
            let mut details = json!({ "source": "direct" });
            if let Some(seconds) = self.retry_after_seconds {
                details["retryAfterSeconds"] = json!(seconds);
            }
            return Err(DevupError::with_details(
                ErrorCode::DevupFigmaRateLimited,
                "Figma request rate limit reached.",
                true,
                details,
            ));
        }
        Err(DevupError::new(
            ErrorCode::DevupSnapshotUnsupported,
            "answered — the collection got past the allowance",
            false,
        ))
    }
}

async fn export(upstream: Arc<dyn FigmaUpstream>) -> anyhow::Result<CallToolResult> {
    let server = DevupServer::new(Services::new(Arc::new(ConnectedAuth), upstream));
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let arguments: Map<String, Value> = json!({
        "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-2",
        "sourcePolicy": "direct"
    })
    .as_object()
    .cloned()
    .unwrap();
    let result = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_export".to_owned()).with_arguments(arguments),
        )
        .await?;
    client.cancel().await?;
    task.await??;
    Ok(result)
}

/// The refusal asks to be waited out — it is marked retryable and often names
/// the seconds. Honouring that turns a lost collection into a slow one.
#[tokio::test(start_paused = true)]
async fn a_refused_call_is_waited_out_rather_than_ending_the_collection() -> anyhow::Result<()> {
    let upstream = Arc::new(SpentAllowance {
        refusals: AtomicUsize::new(2),
        attempts: AtomicUsize::new(0),
        retry_after_seconds: Some(30),
    });

    // Whatever the collection then reports is beside the point here; what is
    // being watched is how many times the refusal was answered.
    let _ = export(upstream.clone()).await;

    assert_eq!(
        upstream.refusals.load(Ordering::SeqCst),
        0,
        "both refusals should have been answered, not surrendered to"
    );
    assert!(
        upstream.attempts.load(Ordering::SeqCst) > 2,
        "waiting out both refusals takes a third call, and the collection carries on from there"
    );
    Ok(())
}

/// Bounded, because an allowance that is genuinely gone must be reported. Four
/// refusals outlast three attempts, and the fourth is never made.
#[tokio::test(start_paused = true)]
async fn an_allowance_that_stays_gone_is_reported_rather_than_waited_on_forever()
-> anyhow::Result<()> {
    let upstream = Arc::new(SpentAllowance {
        refusals: AtomicUsize::new(9),
        attempts: AtomicUsize::new(0),
        retry_after_seconds: None,
    });

    // Whatever the collection then reports is beside the point here; what is
    // being watched is how many times the refusal was answered.
    let _ = export(upstream.clone()).await;

    assert_eq!(
        upstream.attempts.load(Ordering::SeqCst),
        3,
        "three attempts and then the truth"
    );
    Ok(())
}
