//! `responsiveTsx` over frames that are not one screen costs nothing.
//!
//! The refusal used to live in projection, which runs after the collection it
//! needs. Asking for `responsiveTsx` over two frames of a Braillify Studio
//! Section spent 77 seconds and ten Figma reads before answering that the
//! snapshot had no mergeable breakpoint frames — a verdict the Section index
//! already held, because it lists every candidate's name and width.
//!
//! The index is in hand before the second call collects anything, so the
//! answer is due there. What this pins is the cost: the refusal arrives
//! without a single further read.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use devup_mcp::server::{DevupAuth, DevupServer, Services};
use devup_mcp_figma::{
    AuthStatus, BuiltinScript, DevupError, ErrorCode, FigmaUpstream, ReadToolCall, UpstreamResult,
};
use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Map, Value, json};

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

/// A Section holding two different screens at two different widths — the
/// shape a caller reaches for when they want one responsive module and the
/// Section does not hold one.
fn section_index() -> UpstreamResult {
    UpstreamResult {
        raw: json!({
            "fileKey": "FileKey123", "version": null, "rootIds": ["10:1"],
            "nodes": [
                {"id": "10:1", "type": "SECTION", "fields": {
                    "name": "로그인 화면", "parentId": "0:1",
                    "childrenIds": ["10:2", "10:3"], "visible": true,
                    "projectionTruncated": false,
                    "absoluteBoundingBox": {"x": 0, "y": 0, "width": 4000, "height": 2000}
                }, "extra": {}, "fieldErrors": {}},
                {"id": "10:2", "type": "FRAME", "fields": {
                    "name": "AUTH-01", "parentId": "10:1", "childrenIds": [], "visible": true,
                    "directChildCount": 0, "subtreeNodeCount": 1,
                    "estimatedSerializedBytes": 1000,
                    "absoluteBoundingBox": {"x": 0, "y": 0, "width": 1920, "height": 1080}
                }, "extra": {}, "fieldErrors": {}},
                {"id": "10:3", "type": "FRAME", "fields": {
                    "name": "AUTH-02", "parentId": "10:1", "childrenIds": [], "visible": true,
                    "directChildCount": 0, "subtreeNodeCount": 1,
                    "estimatedSerializedBytes": 1000,
                    "absoluteBoundingBox": {"x": 2100, "y": 0, "width": 375, "height": 812}
                }, "extra": {}, "fieldErrors": {}}
            ], "diagnostics": []
        }),
    }
}

/// Counts every read, and refuses to collect: reaching the collection at all
/// is the defect, so it is made loud rather than merely counted.
#[derive(Debug, Default)]
struct IndexOnlyUpstream(AtomicUsize);

#[async_trait]
impl FigmaUpstream for IndexOnlyUpstream {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".to_owned()])
    }
    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        match call {
            ReadToolCall::Snapshot {
                script: BuiltinScript::FastSnapshotEnvelope,
                ..
            } => Ok(section_index()),
            _ => Err(DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "the family was decided from the index; nothing else may be collected",
                false,
            )),
        }
    }
}

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    arguments: Value,
) -> anyhow::Result<(bool, Value)> {
    let arguments: Map<String, Value> = arguments.as_object().cloned().unwrap();
    let result = client
        .call_tool(CallToolRequestParams::new("devup_figma_export").with_arguments(arguments))
        .await?;
    Ok((
        result.is_error == Some(true),
        result
            .structured_content
            .expect("structured content either way"),
    ))
}

#[tokio::test]
async fn responsive_tsx_over_unrelated_frames_is_refused_without_collecting() -> anyhow::Result<()>
{
    let upstream = Arc::new(IndexOnlyUpstream::default());
    let server = DevupServer::new(Services::new(Arc::new(ConnectedAuth), upstream.clone()));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let url = "https://www.figma.com/design/FileKey123/Fixture?node-id=10-1";

    let (errored, selection) = call(&client, json!({"url": url, "outputs": ["tsx"]})).await?;
    assert!(!errored, "{selection}");
    assert_eq!(selection["status"], "selection_required");
    let artifact_id = selection["cache"]["artifactId"]
        .as_str()
        .expect("the index is an artifact")
        .to_owned();
    let after_index = upstream.0.load(Ordering::SeqCst);

    let (errored, refusal) = call(
        &client,
        json!({
            "artifactId": artifact_id,
            "frameIds": ["10:2", "10:3"],
            "outputs": ["responsiveTsx"],
        }),
    )
    .await?;

    // The defect, asserted before anything about wording: the same refusal
    // used to arrive only after the frames had been collected in full.
    assert_eq!(
        upstream.0.load(Ordering::SeqCst),
        after_index,
        "the refusal must not cost a single Figma read"
    );
    assert!(
        errored,
        "two unrelated screens are not one responsive module"
    );
    let text = refusal.to_string();
    assert!(
        text.contains("responsiveTsx"),
        "the refusal names the output it refused: {text}"
    );
    assert!(
        text.contains("Nothing was collected"),
        "the refusal says what it did not spend: {text}"
    );

    client.cancel().await?;
    task.await??;
    Ok(())
}
