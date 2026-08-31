use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
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
            } => Ok(section_envelope()),
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
    assert_eq!(
        selection["selection"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|candidate| candidate["node"]["nodeId"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["10:3", "10:2"]
    );
    assert_eq!(upstream.0.load(Ordering::SeqCst), 1);
    let artifact_id = selection["cache"]["artifactId"].as_str().unwrap();

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
        ["10:2", "10:3"]
    );
    assert!(
        selected["frames"][0]["tsx"]
            .as_str()
            .unwrap()
            .contains("Second")
    );
    assert_eq!(selected["frames"][0]["sourceMap"]["version"], 1);
    assert!(
        selected["frames"][0]["sourceMap"]["entries"]
            .as_array()
            .is_some_and(|entries| entries
                .iter()
                .any(|entry| entry["nodeId"] == "10:2" && entry["property"] == "type"))
    );
    assert_eq!(selected["cache"]["cacheHit"], true);
    assert_eq!(upstream.0.load(Ordering::SeqCst), 1);

    let all = call(
        &client,
        json!({
            "artifactId": artifact_id,
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
    assert_eq!(upstream.0.load(Ordering::SeqCst), 1);

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

fn section_envelope() -> UpstreamResult {
    let mut envelope = json!({
        "schemaVersion": 1,
        "source": {"fileKey": "FileKey123", "rootId": "10:1"},
        "snapshot": {
            "fileKey": "FileKey123", "version": "v1", "rootIds": ["10:1"],
            "nodes": [
                {"id": "10:1", "type": "SECTION", "fields": {
                    "name": "Proofread states", "parentId": "0:1",
                    "childrenIds": ["10:2", "10:3", "10:4"],
                    "absoluteBoundingBox": {"x": 0, "y": 0, "width": 1200, "height": 1000}
                }, "extra": {}, "fieldErrors": {}},
                {"id": "10:2", "type": "FRAME", "fields": {
                    "name": "Second", "parentId": "10:1", "childrenIds": [], "visible": true,
                    "absoluteBoundingBox": {"x": 500, "y": 120, "width": 360, "height": 740},
                    "layoutMode": "VERTICAL", "width": 360, "height": 740
                }, "extra": {}, "fieldErrors": {}},
                {"id": "10:3", "type": "FRAME", "fields": {
                    "name": "First", "parentId": "10:1", "childrenIds": [], "visible": true,
                    "absoluteBoundingBox": {"x": 100, "y": 120, "width": 360, "height": 740},
                    "layoutMode": "VERTICAL", "width": 360, "height": 740
                }, "extra": {}, "fieldErrors": {}},
                {"id": "10:4", "type": "TEXT", "fields": {
                    "name": "Annotation", "parentId": "10:1", "childrenIds": [], "visible": true,
                    "absoluteBoundingBox": {"x": 100, "y": 900, "width": 200, "height": 30},
                    "characters": "Do not export"
                }, "extra": {}, "fieldErrors": {}}
            ], "diagnostics": []
        },
        "resources": {
            "collections": [], "variables": [], "styles": [],
            "usedRemoteVariables": [], "usedVariableIds": [], "usedStyleIds": [],
            "localComplete": false, "usedRemoteComplete": true, "unresolved": []
        },
        "integrity": {"nodeCount": 4, "variableRefCount": 0, "styleRefCount": 0, "utf8Bytes": 0}
    });
    let bytes = loop {
        let bytes = serde_json::to_vec(&envelope).unwrap();
        if envelope["integrity"]["utf8Bytes"] == bytes.len() as u64 {
            break bytes;
        }
        envelope["integrity"]["utf8Bytes"] = Value::from(bytes.len());
    };
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    push_chunk(&mut png, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
    let mut chunk = Vec::with_capacity(bytes.len() + 8);
    chunk.extend_from_slice(&0_u32.to_be_bytes());
    chunk.extend_from_slice(&1_u32.to_be_bytes());
    chunk.extend_from_slice(&bytes);
    push_chunk(&mut png, b"duVp", &chunk);
    push_chunk(
        &mut png,
        b"IDAT",
        &[0x78, 1, 1, 5, 0, 0xfa, 0xff, 0, 0, 0, 0, 0, 5, 0, 1],
    );
    push_chunk(&mut png, b"IEND", &[]);
    let descriptor = json!({
        "kind": "devupFastSnapshotDescriptor", "schemaVersion": 1, "rootId": "10:1",
        "nodeCount": 4, "variableRefCount": 0, "styleRefCount": 0,
        "utf8Bytes": bytes.len(), "chunkCount": 1
    });
    UpstreamResult {
        raw: json!({"content": [
            {"type": "text", "text": descriptor.to_string()},
            {"type": "image", "data": STANDARD.encode(png), "mimeType": "image/png"}
        ]}),
    }
}

fn push_chunk(output: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    output.extend_from_slice(&(data.len() as u32).to_be_bytes());
    output.extend_from_slice(kind);
    output.extend_from_slice(data);
    let mut crc_input = kind.to_vec();
    crc_input.extend_from_slice(data);
    output.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}
