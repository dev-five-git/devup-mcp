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

#[derive(Debug)]
struct FastFixtureUpstream {
    calls: AtomicUsize,
    partial: bool,
    lossy: bool,
}

impl FastFixtureUpstream {
    fn complete() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            partial: false,
            lossy: false,
        }
    }

    fn partial() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            partial: true,
            lossy: false,
        }
    }

    fn lossy() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            partial: false,
            lossy: true,
        }
    }
}

#[async_trait]
impl FigmaUpstream for FastFixtureUpstream {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".to_owned()])
    }

    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match call {
            ReadToolCall::Snapshot {
                script: BuiltinScript::FastSnapshotEnvelope,
                ..
            } => Ok(fast_envelope_result(self.partial, self.lossy)),
            ReadToolCall::AssetExport {
                version, request, ..
            } => Ok(asset_export_result(version.as_deref(), &request)),
            _ => Err(DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "composite export must stay on the one-call fast path",
                false,
            )),
        }
    }
}

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &str,
    arguments: Value,
) -> anyhow::Result<Value> {
    let arguments: Map<String, Value> = arguments.as_object().cloned().unwrap();
    let result = client
        .call_tool(CallToolRequestParams::new(name.to_owned()).with_arguments(arguments))
        .await?;
    Ok(result.structured_content.expect("structured tool output"))
}

#[tokio::test]
async fn one_acquisition_projects_all_outputs_and_artifact_reuse_is_zero_call() -> anyhow::Result<()>
{
    let upstream = Arc::new(FastFixtureUpstream::complete());
    let server = DevupServer::new(Services::new(Arc::new(ConnectedAuth), upstream.clone()));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let url = "https://www.figma.com/design/FileKey123/Fixture?node-id=1-2";

    let first = call(
        &client,
        "devup_figma_export",
        json!({
            "url": url,
            "outputs": ["tsx", "devupJson", "rawSnapshot", "sourceMap", "assetManifest"],
            "scope": "node",
            "sourcePolicy": "direct",
            "includeDiagnostics": true
        }),
    )
    .await?;

    assert_eq!(first["status"], "complete");
    assert_eq!(first["collection"]["figmaToolCalls"], 1);
    assert_eq!(first["cache"]["cacheHit"], false);
    assert!(first["cache"]["artifactId"].as_str().is_some());
    assert!(first["tsx"].as_str().unwrap().contains("$primary"));
    assert!(first["devupJson"].as_str().unwrap().contains("\"primary\""));
    assert_eq!(first["rawSnapshot"]["roots"], json!(["1:2"]));
    assert_eq!(first["sourceMap"]["version"], 1);
    assert_eq!(
        first["assetManifest"]["assets"][0]["assetId"],
        "1:2:fills:1"
    );
    assert_eq!(first["assetManifest"]["assets"][0]["status"], "available");
    assert!(first["sourceMap"]["tsx"].as_array().is_some_and(|entries| {
        entries.iter().any(|entry| {
            entry["nodeId"] == "1:2" && entry["property"] == "fills" && entry["variableId"] == "v"
        })
    }));
    assert!(
        first["sourceMap"]["devupJson"]
            .as_array()
            .is_some_and(|entries| entries.iter().any(|entry| {
                entry["jsonPointer"] == "/theme/colors/default/primary"
                    && entry["variableId"] == "v"
            }))
    );
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 1);

    let artifact_id = first["cache"]["artifactId"].as_str().unwrap();
    let projected = call(
        &client,
        "devup_figma_export",
        json!({
            "artifactId": artifact_id,
            "outputs": ["tsx"],
            "rootLayout": "embedded"
        }),
    )
    .await?;
    assert_eq!(projected["cache"]["cacheHit"], true);
    assert_eq!(projected["cache"]["artifactId"], artifact_id);
    assert!(projected.get("devupJson").is_none());
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 1);

    let ui_wrapper = call(
        &client,
        "devup_figma_to_ui",
        json!({"url": url, "sourcePolicy": "direct"}),
    )
    .await?;
    let json_wrapper = call(
        &client,
        "devup_figma_to_json",
        json!({"url": url, "scope": "node", "sourcePolicy": "direct"}),
    )
    .await?;
    assert_eq!(ui_wrapper["cache"]["cacheHit"], true);
    assert_eq!(json_wrapper["cache"]["cacheHit"], true);
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 1);

    let refreshed = call(
        &client,
        "devup_figma_export",
        json!({
            "url": url,
            "outputs": ["rawSnapshot"],
            "scope": "node",
            "sourcePolicy": "direct",
            "refresh": true
        }),
    )
    .await?;
    assert_eq!(refreshed["cache"]["cacheHit"], false);
    assert_ne!(refreshed["cache"]["artifactId"], artifact_id);
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 2);

    client.cancel().await?;
    task.await??;
    Ok(())
}

#[tokio::test]
async fn artifact_reuse_rejects_file_theme_beyond_captured_scope() -> anyhow::Result<()> {
    let upstream = Arc::new(FastFixtureUpstream::complete());
    let server = DevupServer::new(Services::new(Arc::new(ConnectedAuth), upstream.clone()));
    let (server_transport, client_transport) = tokio::io::duplex(128 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let acquired = call(
        &client,
        "devup_figma_export",
        json!({
            "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-2",
            "outputs": ["tsx"],
            "scope": "node",
            "sourcePolicy": "direct"
        }),
    )
    .await?;
    let artifact_id = acquired["cache"]["artifactId"].as_str().unwrap();
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 1);

    let incompatible = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_export").with_arguments(
                json!({
                    "artifactId": artifact_id,
                    "outputs": ["devupJson"],
                    "scope": "file"
                })
                .as_object()
                .cloned()
                .unwrap(),
            ),
        )
        .await;

    let error = incompatible.expect_err("node artifact must not impersonate file theme capture");
    assert!(
        error.to_string().contains("DEVUP_FIGMA_HANDOFF_INVALID"),
        "unexpected capability error: {error}"
    );
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 1);

    client.cancel().await?;
    task.await??;
    Ok(())
}

#[tokio::test]
async fn explicit_asset_request_exports_once_and_returns_validated_binary() -> anyhow::Result<()> {
    let upstream = Arc::new(FastFixtureUpstream::complete());
    let server = DevupServer::new(Services::new(Arc::new(ConnectedAuth), upstream.clone()));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let result = call(
        &client,
        "devup_figma_export",
        json!({
            "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-2",
            "outputs": ["tsx", "assetManifest"],
            "sourcePolicy": "direct",
            "assetRequests": [{"assetId":"1:2:fills:1","format":"png","scale":2}]
        }),
    )
    .await?;

    assert_eq!(result["status"], "complete");
    assert_eq!(result["collection"]["figmaToolCalls"], 2);
    assert_eq!(result["assetManifest"]["assets"][0]["status"], "exported");
    assert_eq!(
        result["assetManifest"]["assets"][0]["dataBase64"],
        STANDARD.encode(b"synthetic-png")
    );
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 2);

    client.cancel().await?;
    task.await??;
    Ok(())
}

#[tokio::test]
async fn artifact_reuse_rejects_a_different_asset_format_or_scale() -> anyhow::Result<()> {
    let upstream = Arc::new(FastFixtureUpstream::complete());
    let server = DevupServer::new(Services::new(Arc::new(ConnectedAuth), upstream.clone()));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let acquired = call(
        &client,
        "devup_figma_export",
        json!({
            "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-2",
            "outputs": ["assetManifest"],
            "sourcePolicy": "direct",
            "assetRequests": [{"assetId":"1:2:fills:1","format":"png","scale":2}]
        }),
    )
    .await?;
    let artifact_id = acquired["cache"]["artifactId"].as_str().unwrap();
    assert_eq!(acquired["cache"]["capabilities"]["assetCaptureCount"], 1);
    assert!(!serde_json::to_string(&acquired["cache"]["capabilities"])?.contains("1:2:fills:1"));
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 2);

    for request in [
        json!({"assetId":"1:2:fills:1","format":"svg","scale":2}),
        json!({"assetId":"1:2:fills:1","format":"png","scale":1}),
    ] {
        let reused = client
            .call_tool(
                CallToolRequestParams::new("devup_figma_export").with_arguments(
                    json!({
                        "artifactId": artifact_id,
                        "outputs": ["assetManifest"],
                        "assetRequests": [request]
                    })
                    .as_object()
                    .cloned()
                    .unwrap(),
                ),
            )
            .await;
        let error = reused.expect_err("asset capture reuse requires the exact format and scale");
        assert!(
            error.to_string().contains("DEVUP_FIGMA_HANDOFF_INVALID"),
            "unexpected capture mismatch error: {error}"
        );
    }
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 2);

    client.cancel().await?;
    task.await??;
    Ok(())
}

#[tokio::test]
async fn strict_export_rejects_partial_payload_before_projection() -> anyhow::Result<()> {
    let upstream = Arc::new(FastFixtureUpstream::partial());
    let server = DevupServer::new(Services::new(Arc::new(ConnectedAuth), upstream));
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let result = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_export").with_arguments(
                json!({
                    "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-2",
                    "outputs": ["rawSnapshot"],
                    "sourcePolicy": "direct",
                    "strict": true
                })
                .as_object()
                .cloned()
                .unwrap(),
            ),
        )
        .await;

    let error = result.expect_err("strict partial export must fail");
    assert!(
        error.to_string().contains("partial"),
        "unexpected strict error: {error}"
    );
    client.cancel().await?;
    task.await??;
    Ok(())
}

#[tokio::test]
async fn strict_tsx_export_rejects_lossy_projection() -> anyhow::Result<()> {
    let upstream = Arc::new(FastFixtureUpstream::lossy());
    let server = DevupServer::new(Services::new(Arc::new(ConnectedAuth), upstream));
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let output_path = std::env::temp_dir().join(format!(
        "devup-mcp-strict-lossy-{}-{}.tsx",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));

    let result = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_export").with_arguments(
                json!({
                    "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-2",
                    "outputs": ["tsx"],
                    "sourcePolicy": "direct",
                    "strict": true,
                    "outputPaths": {"tsx": output_path.to_string_lossy()}
                })
                .as_object()
                .cloned()
                .unwrap(),
            ),
        )
        .await;

    let error = result.expect_err("strict lossy export must fail");
    assert!(
        error.to_string().contains("lossy"),
        "unexpected strict error: {error}"
    );
    let wrote_rejected_output = output_path.exists();
    if wrote_rejected_output {
        std::fs::remove_file(&output_path)?;
    }
    assert!(
        !wrote_rejected_output,
        "strict rejection must not write output files"
    );
    client.cancel().await?;
    task.await??;
    Ok(())
}

fn fast_envelope_result(partial: bool, lossy: bool) -> UpstreamResult {
    let mut envelope = json!({
        "schemaVersion": 1,
        "source": {"fileKey": "FileKey123", "rootId": "1:2"},
        "snapshot": {
            "fileKey": "FileKey123",
            "version": "v1",
            "rootIds": ["1:2"],
            "nodes": [{
                "id": "1:2",
                "type": "FRAME",
                "fields": {
                    "name": "Synthetic",
                    "childrenIds": [],
                    "layoutMode": "VERTICAL",
                    "width": 320,
                    "height": 240,
                    "fills": [{
                        "type": "SOLID",
                        "color": {"r": 0, "g": 0.4, "b": 1, "a": 1},
                        "boundVariables": {"color": {"type": "VARIABLE_ALIAS", "id": "v"}}
                    }, {"type":"IMAGE","imageHash":"image-hash-123","scaleMode":"FILL"}],
                    "boundVariables": {"fills": [{"type": "VARIABLE_ALIAS", "id": "v"}]}
                },
                "extra": {},
                "fieldErrors": {}
            }],
            "diagnostics": []
        },
        "resources": {
            "collections": [{
                "id": "c", "name": "Theme", "defaultModeId": "m",
                "modes": [{"modeId": "m", "name": "Default"}]
            }],
            "variables": [{
                "id": "v", "name": "primary", "resolvedType": "COLOR",
                "variableCollectionId": "c", "codeSyntax": {"WEB": "primary"},
                "valuesByMode": {"m": {"r": 0, "g": 0.4, "b": 1, "a": 1}}
            }],
            "styles": [],
            "usedVariableIds": ["v"],
            "usedStyleIds": [],
            "usedRemoteVariables": [],
            "localComplete": false,
            "usedRemoteComplete": true,
            "unresolved": []
        },
        "integrity": {
            "nodeCount": 1,
            "variableRefCount": 1,
            "styleRefCount": 0,
            "utf8Bytes": 0
        }
    });
    if partial {
        envelope["resources"]["variables"] = json!([]);
        envelope["resources"]["unresolved"] = json!([{
            "id": "v", "kind": "variable", "reason": "fixture-unresolved"
        }]);
    }
    if lossy {
        envelope["snapshot"]["nodes"][0]["fields"]["isMask"] = json!(true);
        envelope["snapshot"]["nodes"][0]["fields"]["effects"] =
            json!([{"type": "BACKGROUND_BLUR"}]);
    }
    let envelope_bytes = loop {
        let bytes = serde_json::to_vec(&envelope).unwrap();
        if envelope["integrity"]["utf8Bytes"] == bytes.len() as u64 {
            break bytes;
        }
        envelope["integrity"]["utf8Bytes"] = Value::from(bytes.len());
    };

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    push_png_chunk(&mut png, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
    let mut payload = Vec::with_capacity(envelope_bytes.len() + 8);
    payload.extend_from_slice(&0_u32.to_be_bytes());
    payload.extend_from_slice(&1_u32.to_be_bytes());
    payload.extend_from_slice(&envelope_bytes);
    push_png_chunk(&mut png, b"duVp", &payload);
    push_png_chunk(
        &mut png,
        b"IDAT",
        &[
            0x78, 0x01, 0x01, 0x05, 0x00, 0xfa, 0xff, 0, 0, 0, 0, 0, 5, 0, 1,
        ],
    );
    push_png_chunk(&mut png, b"IEND", &[]);
    let descriptor = json!({
        "kind": "devupFastSnapshotDescriptor",
        "schemaVersion": 1,
        "rootId": "1:2",
        "nodeCount": 1,
        "variableRefCount": 1,
        "styleRefCount": 0,
        "utf8Bytes": envelope_bytes.len(),
        "chunkCount": 1
    });
    UpstreamResult {
        raw: json!({"content": [
            {"type": "text", "text": descriptor.to_string()},
            {"type": "image", "data": STANDARD.encode(png), "mimeType": "image/png"}
        ]}),
    }
}

fn asset_export_result(
    version: Option<&str>,
    request: &devup_mcp_figma::AssetRequest,
) -> UpstreamResult {
    let bytes = b"synthetic-png";
    let descriptor = json!({
        "kind":"devupAssetExport","fileKey":"FileKey123","version":version,
        "assetId":request.asset_id,"nodeId":request.node_id,"field":request.field,
        "imageHash":request.image_hash,"format":request.format,"scale":request.scale,
        "status":"exported","byteLength":bytes.len(),
        "sha256":"294ad7145322ec19f8250cca8480a933f1ce8c9e2ad1038e7ae8930d55a6598a"
    });
    UpstreamResult {
        raw: json!({"content":[
            {"type":"text","text":descriptor.to_string()},
            {"type":"image","data":STANDARD.encode(bytes),"mimeType":"image/png"}
        ]}),
    }
}

fn push_png_chunk(output: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    output.extend_from_slice(&(data.len() as u32).to_be_bytes());
    output.extend_from_slice(chunk_type);
    output.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(chunk_type);
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
