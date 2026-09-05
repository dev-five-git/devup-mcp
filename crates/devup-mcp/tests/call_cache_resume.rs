//! A refused collection keeps the reads it already paid for.
//!
//! Collection is all-or-nothing: one refusal ends it and every read it had
//! already made is discarded. Against a metered allowance that is not merely
//! wasteful, it is unrecoverable — one page-height screen was measured at over
//! a hundred and ten reads against a seat allowed two hundred a day, so three
//! attempts on three days each start from nothing and each end in the same
//! place. The allowance spends down and the work never accumulates.
//!
//! What is watched here is that the second attempt does not buy what the first
//! one already has.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use devup_mcp::server::{DevupAuth, DevupServer, Services};
use devup_mcp_figma::{
    AuthStatus, BuiltinScript, DevupError, ErrorCode, FigmaUpstream, ReadToolCall, UpstreamResult,
};
use rmcp::{ServiceExt, model::CallToolRequestParams};
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

/// Answers the Section index and refuses everything after it, the way an
/// allowance that runs out partway through a collection does. The counters are
/// shared between attempts because the allowance is.
#[derive(Default)]
struct AllowanceRunsOut {
    index_calls: AtomicUsize,
    total_calls: AtomicUsize,
}

#[async_trait]
impl FigmaUpstream for AllowanceRunsOut {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".to_owned()])
    }

    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        self.total_calls.fetch_add(1, Ordering::SeqCst);
        match call {
            ReadToolCall::Snapshot {
                script: BuiltinScript::SectionIndex,
                ..
            } => {
                self.index_calls.fetch_add(1, Ordering::SeqCst);
                Ok(section_index_result())
            }
            _ => Err(DevupError::with_details(
                ErrorCode::DevupFigmaRateLimited,
                "You've reached the Figma MCP tool call limit for your Full seat on the Professional plan.",
                true,
                json!({"source": "direct"}),
            )),
        }
    }
}

async fn attempt(upstream: Arc<AllowanceRunsOut>, cache: PathBuf) -> anyhow::Result<()> {
    let server = DevupServer::new(Services::with_call_cache_dir(
        Arc::new(ConnectedAuth),
        upstream,
        Some(cache),
    ));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let arguments: Map<String, Value> = json!({
        "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=10-1",
        "allScreens": true,
        "outputs": ["rawSnapshot"],
        "sourcePolicy": "direct"
    })
    .as_object()
    .cloned()
    .expect("arguments object");

    // It cannot succeed: the reads after the index are all refused. What the
    // attempt returns is beside the point; what it spent is not.
    let _ = client
        .call_tool(CallToolRequestParams::new("devup_figma_export").with_arguments(arguments))
        .await;

    client.cancel().await?;
    task.abort();
    Ok(())
}

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "devup-resume-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or_default()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("scratch directory");
    path
}

/// Paused, because a refusal now stands the pacer down for a whole window and
/// three attempts of that is three minutes of real waiting for nothing.
#[tokio::test(start_paused = true)]
async fn a_second_attempt_does_not_buy_what_the_first_one_banked() -> anyhow::Result<()> {
    let cache = scratch("banked");
    let upstream = Arc::new(AllowanceRunsOut::default());

    attempt(upstream.clone(), cache.clone()).await?;
    let first_total = upstream.total_calls.swap(0, Ordering::SeqCst);
    let first_index = upstream.index_calls.swap(0, Ordering::SeqCst);

    assert_eq!(
        first_index, 1,
        "the first attempt has to buy the index once"
    );
    assert!(
        std::fs::read_dir(&cache)
            .expect("cache directory")
            .flatten()
            .any(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json")),
        "a read that succeeded must be banked"
    );

    attempt(upstream.clone(), cache.clone()).await?;
    let second_total = upstream.total_calls.load(Ordering::SeqCst);
    let second_index = upstream.index_calls.load(Ordering::SeqCst);

    assert_eq!(
        second_index, 0,
        "the index was already paid for, so it must be replayed rather than bought again"
    );
    assert!(
        second_total < first_total,
        "the second attempt must spend less: {second_total} against {first_total}"
    );

    let _ = std::fs::remove_dir_all(&cache);
    Ok(())
}

/// Without a directory the bank does not exist, and every attempt pays again.
#[tokio::test(start_paused = true)]
async fn nothing_is_banked_unless_a_directory_was_named() -> anyhow::Result<()> {
    let upstream = Arc::new(AllowanceRunsOut::default());

    let run = |upstream: Arc<AllowanceRunsOut>| async move {
        let server = DevupServer::new(Services::with_call_cache_dir(
            Arc::new(ConnectedAuth),
            upstream,
            None,
        ));
        let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
        let task = tokio::spawn(async move {
            server.serve(server_transport).await?.waiting().await?;
            anyhow::Ok(())
        });
        let client = ().serve(client_transport).await?;
        let arguments: Map<String, Value> = json!({
            "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=10-1",
            "allScreens": true,
            "outputs": ["rawSnapshot"],
            "sourcePolicy": "direct"
        })
        .as_object()
        .cloned()
        .expect("arguments object");
        let _ = client
            .call_tool(CallToolRequestParams::new("devup_figma_export").with_arguments(arguments))
            .await;
        client.cancel().await?;
        task.abort();
        anyhow::Ok(())
    };

    run(upstream.clone()).await?;
    let first_index = upstream.index_calls.swap(0, Ordering::SeqCst);
    run(upstream.clone()).await?;
    let second_index = upstream.index_calls.load(Ordering::SeqCst);

    assert_eq!(first_index, 1, "the first attempt buys the index");
    assert_eq!(
        second_index, 1,
        "with no bank named, the second attempt buys it again"
    );
    Ok(())
}

fn section_index_result() -> UpstreamResult {
    UpstreamResult {
        raw: json!({
            "fileKey": "FileKey123", "version": null, "rootIds": ["10:1"],
            "nodes": [
                {"id": "10:1", "type": "SECTION", "fields": {
                    "name": "Proofread states", "parentId": "0:1", "childrenIds": ["10:2", "10:3"],
                    "visible": true, "projectionTruncated": false,
                    "absoluteBoundingBox": {"x": 0, "y": 0, "width": 1200, "height": 1000}
                }, "extra": {}, "fieldErrors": {}},
                {"id": "10:2", "type": "FRAME", "fields": {
                    "name": "Second", "parentId": "10:1", "childrenIds": [], "visible": true,
                    "directChildCount": 0, "subtreeNodeCount": 1, "estimatedSerializedBytes": 1000,
                    "absoluteBoundingBox": {"x": 500, "y": 120, "width": 360, "height": 740}
                }, "extra": {}, "fieldErrors": {}},
                {"id": "10:3", "type": "FRAME", "fields": {
                    "name": "First", "parentId": "10:1", "childrenIds": [], "visible": true,
                    "directChildCount": 0, "subtreeNodeCount": 1, "estimatedSerializedBytes": 1000,
                    "absoluteBoundingBox": {"x": 100, "y": 120, "width": 360, "height": 740}
                }, "extra": {}, "fieldErrors": {}}
            ], "diagnostics": []
        }),
    }
}
