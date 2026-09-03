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

#[derive(Debug, Default)]
struct SectionUpstream(AtomicUsize);

#[async_trait]
impl FigmaUpstream for SectionUpstream {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".to_owned()])
    }
    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        match call {
            ReadToolCall::Snapshot {
                script: BuiltinScript::FastSnapshotEnvelope,
                ..
            } => Ok(compact_section_index_result()),
            ReadToolCall::Snapshot {
                script: BuiltinScript::MultiRootSnapshotEnvelope,
                root_ids: Some(root_ids),
                ..
            } => Ok(multi_root_envelope(&root_ids)),
            _ => Err(DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "section export must use one fast acquisition",
                false,
            )),
        }
    }
}

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    arguments: Value,
) -> anyhow::Result<Value> {
    let arguments: Map<String, Value> = arguments.as_object().cloned().unwrap();
    let result = client
        .call_tool(CallToolRequestParams::new("devup_figma_export").with_arguments(arguments))
        .await?;
    Ok(result.structured_content.expect("structured output"))
}

#[tokio::test]
async fn section_requires_selection_then_exports_requested_or_all_screens_from_one_artifact()
-> anyhow::Result<()> {
    let upstream = Arc::new(SectionUpstream::default());
    let server = DevupServer::new(Services::new(Arc::new(ConnectedAuth), upstream.clone()));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let url = "https://www.figma.com/design/FileKey123/Fixture?node-id=10-1";

    let selection = call(
        &client,
        json!({"url": url, "outputs": ["tsx"], "sourcePolicy": "direct"}),
    )
    .await?;
    assert_eq!(selection["status"], "selection_required");
    assert_eq!(selection["targetKind"], "section");
    assert!(selection.get("tsx").is_none());
    assert_eq!(selection["collection"]["figmaToolCalls"], 1);
    assert_eq!(
        selection["selection"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|candidate| candidate["node"]["nodeId"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["10:3", "10:2"]
    );
    assert_eq!(
        selection["nextAction"]["why"],
        "This link is a Section and holds several screens inside. Collecting them all at once exceeds the size limit."
    );
    assert_eq!(
        selection["nextAction"]["how"],
        "Call again with the target screen's canonicalUrl from screens[], or use allScreens:true if you need every screen."
    );
    assert_eq!(
        selection["nextAction"]["doNot"],
        "Do not try to collect the whole Section at once."
    );
    assert_eq!(upstream.0.load(Ordering::SeqCst), 1);
    let artifact_id = selection["cache"]["artifactId"].as_str().unwrap();
    assert_eq!(selection["cache"]["capabilities"]["kind"], "section-index");

    let selected = call(
        &client,
        json!({
            "artifactId": artifact_id,
            "outputs": ["tsx", "sourceMap"],
            "frameIds": ["10:2", "10:3"]
        }),
    )
    .await?;
    assert_eq!(selected["status"], "complete");
    assert_eq!(
        selected["frames"]
            .as_array()
            .unwrap()
            .iter()
            .map(|frame| frame["nodeId"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["10:3", "10:2"]
    );
    assert!(
        selected["frames"][0]["tsx"]
            .as_str()
            .unwrap()
            .contains("First")
    );
    assert_eq!(selected["frames"][0]["sourceMap"]["version"], 1);
    assert!(
        selected["frames"][0]["sourceMap"]["entries"]
            .as_array()
            .is_some_and(|entries| entries
                .iter()
                .any(|entry| entry["nodeId"] == "10:3" && entry["property"] == "type"))
    );
    assert_eq!(selected["cache"]["cacheHit"], false);
    assert_eq!(selected["collection"]["figmaToolCalls"], 2);
    assert_eq!(upstream.0.load(Ordering::SeqCst), 3);
    let selected_artifact_id = selected["cache"]["artifactId"].as_str().unwrap();

    let all = call(
        &client,
        json!({
            "artifactId": selected_artifact_id,
            "outputs": ["tsx"],
            "allScreens": true
        }),
    )
    .await?;
    assert_eq!(
        all["frames"]
            .as_array()
            .unwrap()
            .iter()
            .map(|frame| frame["nodeId"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["10:3", "10:2"]
    );
    assert_eq!(upstream.0.load(Ordering::SeqCst), 3);
    assert_eq!(all["collection"]["figmaToolCalls"], 0);
    assert_eq!(all["cache"]["originCollection"]["figmaToolCalls"], 2);
    assert_eq!(all["cache"]["avoidedFigmaToolCalls"], 2);

    let invalid = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_export").with_arguments(
                json!({
                    "artifactId": artifact_id,
                    "outputs": ["tsx"],
                    "frameIds": ["99:99"]
                })
                .as_object()
                .cloned()
                .unwrap(),
            ),
        )
        .await;
    assert!(invalid.is_err());

    client.cancel().await?;
    task.await??;
    Ok(())
}

#[test]
fn actual_wquw_151_section_fixture_preserves_the_ten_screen_index() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/wquw-151-section.json"))
        .expect("actual WQUW-151 Section fixture");
    assert_eq!(fixture["source"]["fileKey"], "85CgSws3o5XsLv7aAwWJyS");
    assert_eq!(fixture["source"]["nodeId"], "4217:7743");
    assert_eq!(fixture["section"]["name"], "[FR-026] 본연체");
    assert_eq!(fixture["proofreadTargetId"], "3879:35518");
    let candidates = fixture["screenCandidates"]
        .as_array()
        .expect("screen candidates");
    assert_eq!(candidates.len(), 10);
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate["id"].as_str().expect("candidate id"))
            .collect::<Vec<_>>(),
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
}

fn compact_section_index_result() -> UpstreamResult {
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

fn multi_root_envelope(root_ids: &[String]) -> UpstreamResult {
    let nodes = root_ids
        .iter()
        .map(|root_id| match root_id.as_str() {
            "10:2" => json!({"id": "10:2", "type": "FRAME", "fields": {
                "name": "Second", "parentId": "10:1", "childrenIds": [], "visible": true,
                "absoluteBoundingBox": {"x": 500, "y": 120, "width": 360, "height": 740},
                "layoutMode": "VERTICAL", "width": 360, "height": 740
            }, "extra": {}, "fieldErrors": {}}),
            "10:3" => json!({"id": "10:3", "type": "FRAME", "fields": {
                "name": "First", "parentId": "10:1", "childrenIds": [], "visible": true,
                "absoluteBoundingBox": {"x": 100, "y": 120, "width": 360, "height": 740},
                "layoutMode": "VERTICAL", "width": 360, "height": 740
            }, "extra": {}, "fieldErrors": {}}),
            _ => panic!("unexpected selected root"),
        })
        .collect::<Vec<_>>();
    let mut envelope = json!({
        "kind": "devupFastSnapshotEnvelope",
        "schemaVersion": 1,
        "source": {"fileKey": "FileKey123", "rootId": "10:1"},
        "snapshot": {
            "fileKey": "FileKey123", "version": null, "rootIds": root_ids,
            "nodes": nodes, "diagnostics": []
        },
        "resources": {
            "collections": [], "variables": [], "styles": [],
            "usedRemoteVariables": [], "usedVariableIds": [], "usedStyleIds": [],
            "localComplete": false, "usedRemoteComplete": true, "unresolved": []
        },
        "integrity": {"nodeCount": root_ids.len(), "variableRefCount": 0, "styleRefCount": 0, "utf8Bytes": 0}
    });
    let _bytes = loop {
        let bytes = serde_json::to_vec(&envelope).unwrap();
        if envelope["integrity"]["utf8Bytes"] == bytes.len() as u64 {
            break bytes;
        }
        envelope["integrity"]["utf8Bytes"] = Value::from(bytes.len());
    };
    // No binary transport exists any more: fast snapshots are always plain
    // text (`devupFastSnapshotEnvelope`). Omitting the cursor marker node is
    // treated by the decoder as a single, already-complete page.
    UpstreamResult {
        raw: json!({"content": [
            {"type": "text", "text": envelope.to_string()}
        ]}),
    }
}
