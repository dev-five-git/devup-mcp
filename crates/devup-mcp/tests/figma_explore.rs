use std::sync::Arc;

use async_trait::async_trait;
use devup_mcp::server::{DevupAuth, DevupServer, Services};
use devup_mcp_figma::{
    AuthStatus, DevupError, ErrorCode, FigmaUpstream, ReadToolCall, UpstreamResult,
};
use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Map, Value, json};

#[derive(Debug)]
struct Auth(AuthStatus);

#[async_trait]
impl DevupAuth for Auth {
    async fn status(&self) -> Result<AuthStatus, DevupError> {
        Ok(self.0)
    }

    async fn login(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Connected)
    }

    async fn logout(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Disconnected)
    }
}

#[derive(Debug)]
struct ExploreUpstream;

#[async_trait]
impl FigmaUpstream for ExploreUpstream {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".to_owned()])
    }

    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        match call {
            ReadToolCall::ExploreSnapshot { options, .. } => {
                assert!(options.projection_limit >= 50);
                assert_eq!(options.text_preview_limit, 160);
                Ok(UpstreamResult { raw: projection() })
            }
            _ => Err(DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "unexpected test call",
                false,
            )),
        }
    }
}

fn projection() -> Value {
    json!({
        "fileKey": "FileKey123",
        "version": null,
        "rootIds": ["0:1"],
        "nodes": [
            {
                "id": "0:1", "type": "PAGE",
                "fields": {"name": "Phase2", "parentId": null, "childrenIds": [], "x": 0, "y": 0, "width": 1400, "height": 1600, "childCount": 20, "textPreview": "", "projectionTruncated": false},
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:1", "type": "FRAME",
                "fields": {"name": "[FR-026] 본연체", "parentId": "0:1", "childrenIds": [], "x": 0, "y": 0, "width": 1200, "height": 80, "childCount": 1, "textPreview": "본연체"},
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:2", "type": "FRAME",
                "fields": {"name": "A : STORY-F-PROOFREAD", "parentId": "0:1", "childrenIds": [], "x": 0, "y": 120, "width": 360, "height": 740, "childCount": 12, "textPreview": "이야기가 글로 정리되었어요"},
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:3", "type": "FRAME",
                "fields": {"name": "A : STORY-F-PROOFREAD", "parentId": "0:1", "childrenIds": [], "x": 400, "y": 120, "width": 360, "height": 740, "childCount": 13, "textPreview": "공개 설정 나만 보기"},
                "extra": {}, "fieldErrors": {}
            }
        ],
        "diagnostics": []
    })
}

async fn start_client(
    status: AuthStatus,
) -> anyhow::Result<(
    rmcp::service::RunningService<rmcp::RoleClient, ()>,
    tokio::task::JoinHandle<anyhow::Result<()>>,
)> {
    let server = DevupServer::new(Services::new(
        Arc::new(Auth(status)),
        Arc::new(ExploreUpstream),
    ));
    let (server_transport, client_transport) = tokio::io::duplex(128 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    Ok((client, task))
}

fn input(source_policy: &str) -> Map<String, Value> {
    json!({
        "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-1",
        "limit": 50,
        "includeTextPreview": true,
        "sourcePolicy": source_policy
    })
    .as_object()
    .cloned()
    .unwrap()
}

#[tokio::test]
async fn direct_and_host_explore_return_identical_candidate_data() -> anyhow::Result<()> {
    let (client, task) = start_client(AuthStatus::Connected).await?;
    let direct = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_explore").with_arguments(input("direct")),
        )
        .await?
        .structured_content
        .unwrap();
    let start = client
        .call_tool(CallToolRequestParams::new("devup_figma_explore").with_arguments(input("host")))
        .await?
        .structured_content
        .unwrap();
    assert_eq!(start["status"], "needs_figma");
    assert_eq!(start["calls"].as_array().unwrap().len(), 1);
    assert_eq!(start["calls"][0]["tool"], "use_figma");
    let code = start["calls"][0]["arguments"]["code"].as_str().unwrap();
    assert!(code.contains("projectionTruncated"));
    assert!(!code.contains("getVariableByIdAsync"));

    let complete = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_continue").with_arguments(
                json!({
                    "sessionId": start["sessionId"],
                    "callId": start["calls"][0]["callId"],
                    "result": projection()
                })
                .as_object()
                .cloned()
                .unwrap(),
            ),
        )
        .await?
        .structured_content
        .unwrap();

    assert_eq!(direct["status"], "complete");
    assert_eq!(direct["anchor"]["kind"], "heading");
    assert_eq!(direct["count"], 2);
    assert_eq!(direct["candidates"][0]["node"]["nodeId"], "1:2");
    for field in ["anchor", "group", "candidates", "truncated", "diagnostics"] {
        assert_eq!(direct[field], complete[field], "source changed {field}");
    }
    assert_eq!(direct["source"]["kind"], "direct");
    assert_eq!(complete["source"]["kind"], "host");

    client.cancel().await?;
    task.await??;
    Ok(())
}

#[tokio::test]
async fn explore_rejects_missing_node_and_out_of_range_limit() -> anyhow::Result<()> {
    let (client, task) = start_client(AuthStatus::Connected).await?;
    let missing_node = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_explore").with_arguments(
                json!({
                    "url": "https://www.figma.com/design/FileKey123/Fixture",
                    "sourcePolicy": "direct"
                })
                .as_object()
                .cloned()
                .unwrap(),
            ),
        )
        .await;
    assert!(missing_node.is_err());
    let invalid_limit = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_explore").with_arguments(
                json!({
                    "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-1",
                    "limit": 101,
                    "sourcePolicy": "direct"
                })
                .as_object()
                .cloned()
                .unwrap(),
            ),
        )
        .await;
    assert!(invalid_limit.is_err());

    client.cancel().await?;
    task.await??;
    Ok(())
}
