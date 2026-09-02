use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use devup_mcp::server::{DevupAuth, DevupServer, Services};
use devup_mcp_figma::{
    AuthStatus, DevupError, ErrorCode, ExploreOptions, FigmaTarget, FigmaUpstream, ReadToolCall,
    SnapshotChunk, UpstreamResult, explore_snapshot, merge_chunks,
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

#[derive(Debug)]
struct CountingExploreUpstream {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl FigmaUpstream for CountingExploreUpstream {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".to_owned()])
    }

    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        match call {
            ReadToolCall::ExploreSnapshot { .. } => {
                self.calls.fetch_add(1, Ordering::SeqCst);
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
async fn related_nodes_reuse_one_explore_projection_without_changing_the_requested_anchor()
-> anyhow::Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let server = DevupServer::new(Services::new(
        Arc::new(Auth(AuthStatus::Connected)),
        Arc::new(CountingExploreUpstream {
            calls: calls.clone(),
        }),
    ));
    let (server_transport, client_transport) = tokio::io::duplex(128 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let heading = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_explore").with_arguments(input("direct")),
        )
        .await?
        .structured_content
        .unwrap();
    let mut screen_input = input("direct");
    screen_input.insert(
        "url".to_owned(),
        json!("https://www.figma.com/design/FileKey123/Fixture?node-id=1-2"),
    );
    screen_input.insert("limit".to_owned(), json!(10));
    let screen = client
        .call_tool(CallToolRequestParams::new("devup_figma_explore").with_arguments(screen_input))
        .await?
        .structured_content
        .unwrap();

    assert_eq!(heading["anchor"]["nodeId"], "1:1");
    assert_eq!(heading["cache"]["reuseKind"], "miss");
    assert_eq!(heading["collection"]["figmaToolCalls"], 1);
    assert_eq!(screen["anchor"]["nodeId"], "1:2");
    assert_eq!(screen["source"]["nodeId"], "1:2");
    assert_eq!(screen["cache"]["cacheHit"], true);
    assert_eq!(screen["cache"]["reuseKind"], "related-node-superset");
    assert_eq!(screen["cache"]["avoidedFigmaToolCalls"], 1);
    assert_eq!(screen["cache"]["ageSeconds"], 0);
    assert!(screen["cache"]["remainingTtlSeconds"].as_u64().unwrap() > 0);
    assert_eq!(screen["cache"]["originCollection"]["figmaToolCalls"], 1);
    assert_eq!(screen["collection"]["figmaToolCalls"], 0);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let mut different_projection = input("direct");
    different_projection.insert(
        "url".to_owned(),
        json!("https://www.figma.com/design/FileKey123/Fixture?node-id=1-3"),
    );
    different_projection.insert("includeTextPreview".to_owned(), json!(false));
    let different_projection = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_explore").with_arguments(different_projection),
        )
        .await?
        .structured_content
        .unwrap();
    assert_eq!(different_projection["anchor"]["nodeId"], "1:3");
    assert_eq!(different_projection["cache"]["cacheHit"], false);
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    client.cancel().await?;
    task.await??;
    Ok(())
}

#[tokio::test]
async fn refresh_bypasses_an_exact_explore_cache_hit() -> anyhow::Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let server = DevupServer::new(Services::new(
        Arc::new(Auth(AuthStatus::Connected)),
        Arc::new(CountingExploreUpstream {
            calls: calls.clone(),
        }),
    ));
    let (server_transport, client_transport) = tokio::io::duplex(128 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let first = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_explore").with_arguments(input("direct")),
        )
        .await?
        .structured_content
        .unwrap();
    let exact = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_explore").with_arguments(input("direct")),
        )
        .await?
        .structured_content
        .unwrap();
    let mut refreshed_input = input("direct");
    refreshed_input.insert("refresh".to_owned(), json!(true));
    let refreshed = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_explore").with_arguments(refreshed_input),
        )
        .await?
        .structured_content
        .unwrap();

    assert_eq!(first["cache"]["cacheHit"], false);
    assert_eq!(exact["cache"]["cacheHit"], true);
    assert_eq!(exact["cache"]["reuseKind"], "exact");
    assert_eq!(exact["collection"]["figmaToolCalls"], 0);
    assert_eq!(exact["cache"]["originCollection"]["figmaToolCalls"], 1);
    assert_eq!(refreshed["cache"]["cacheHit"], false);
    assert_eq!(refreshed["cache"]["reuseKind"], "miss");
    assert_ne!(
        first["cache"]["artifactId"],
        refreshed["cache"]["artifactId"]
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    client.cancel().await?;
    task.await??;
    Ok(())
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

    // Explore is an intentionally shallow spatial projection. Its candidate data is
    // complete for the operation, while the preserved graph correctly reports that
    // descendants represented by childCount were not included in the snapshot.
    assert_eq!(direct["status"], "complete");
    assert_eq!(complete["status"], "complete");
    assert_eq!(direct["quality"]["acquisition"], "expected-projection");
    assert_eq!(complete["quality"]["acquisition"], "expected-projection");
    assert_eq!(direct["quality"]["projection"], "not-requested");
    assert!(
        !direct["completenessReport"]["snapshot"]["childCountMismatches"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(direct["anchor"]["kind"], "heading");
    assert_eq!(direct["targetKind"], "other");
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
async fn host_explore_accepts_the_public_string_result_contract() -> anyhow::Result<()> {
    let (client, task) = start_client(AuthStatus::Connected).await?;
    let start = client
        .call_tool(CallToolRequestParams::new("devup_figma_explore").with_arguments(input("host")))
        .await?
        .structured_content
        .unwrap();

    let complete = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_continue").with_arguments(
                json!({
                    "sessionId": start["sessionId"],
                    "callId": start["calls"][0]["callId"],
                    "result": projection().to_string()
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
    assert_eq!(complete["count"], 2);
    assert_eq!(complete["source"]["kind"], "host");

    client.cancel().await?;
    task.await??;
    Ok(())
}

#[tokio::test]
async fn completed_host_projection_serves_a_related_node_without_another_handoff()
-> anyhow::Result<()> {
    let (client, task) = start_client(AuthStatus::Connected).await?;
    let start = client
        .call_tool(CallToolRequestParams::new("devup_figma_explore").with_arguments(input("host")))
        .await?
        .structured_content
        .unwrap();
    let completed = client
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
    assert_eq!(completed["status"], "complete");

    let mut related_input = input("host");
    related_input.insert(
        "url".to_owned(),
        json!("https://www.figma.com/design/FileKey123/Fixture?node-id=1-2"),
    );
    let related = client
        .call_tool(CallToolRequestParams::new("devup_figma_explore").with_arguments(related_input))
        .await?
        .structured_content
        .unwrap();

    assert_eq!(related["status"], "complete");
    assert_eq!(related["anchor"]["nodeId"], "1:2");
    assert_eq!(related["source"]["nodeId"], "1:2");
    assert_eq!(related["source"]["kind"], "artifact");
    assert_eq!(related["cache"]["cacheHit"], true);
    assert_eq!(related["cache"]["reuseKind"], "related-node");
    assert_eq!(related["collection"]["figmaToolCalls"], 0);
    assert_eq!(related["cache"]["originCollection"]["figmaToolCalls"], 1);
    assert!(related.get("calls").is_none());

    client.cancel().await?;
    task.await??;
    Ok(())
}

#[tokio::test]
async fn host_explore_unwraps_a_stringified_official_mcp_envelope() -> anyhow::Result<()> {
    let (client, task) = start_client(AuthStatus::Connected).await?;
    let start = client
        .call_tool(CallToolRequestParams::new("devup_figma_explore").with_arguments(input("host")))
        .await?
        .structured_content
        .unwrap();
    let official_result = json!({
        "content": [{"type": "text", "text": projection().to_string()}],
        "isError": false
    });

    let complete = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_continue").with_arguments(
                json!({
                    "sessionId": start["sessionId"],
                    "callId": start["calls"][0]["callId"],
                    "result": official_result.to_string()
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
    assert_eq!(complete["count"], 2);

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

#[test]
fn wquw_151_heading_discovers_the_actual_screen_group() {
    let chunk: SnapshotChunk =
        serde_json::from_str(include_str!("fixtures/wquw-151-neighborhood.json")).unwrap();
    let snapshot = merge_chunks(vec![chunk]).unwrap();
    let target = FigmaTarget::parse(
        "https://www.figma.com/design/85CgSws3o5XsLv7aAwWJyS/Girok?node-id=3879-35481",
    )
    .unwrap();

    let result = explore_snapshot(&snapshot, &target, &ExploreOptions { limit: 50 }).unwrap();
    let ids = result
        .candidates
        .iter()
        .map(|candidate| candidate.node.node_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        ids,
        [
            "3879:36108",
            "3879:36059",
            "3879:35503",
            "3879:35518",
            "3879:35569",
            "3879:36144",
            "3879:35729",
            "3879:35887",
            "3879:35973",
            "3879:35652",
        ]
    );
    assert_eq!(result.anchor.name, "[FR-026] 본연체");
    assert!(
        result
            .candidates
            .iter()
            .any(|candidate| candidate.node.node_id == "3879:35518")
    );
    assert!(!result.truncated);
}
