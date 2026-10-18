#[path = "../common/mod.rs"]
mod common;

use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp::server::{DevupAuth, DevupServer, Services};
use devup_mcp_figma::{
    AuthStatus, BuiltinScript, DevupError, ErrorCode, FigmaUpstream, ReadToolCall, UpstreamResult,
};
use rmcp::{
    ServiceExt,
    model::{
        CallToolRequestParams, CallToolResult, ContentBlock, ReadResourceRequestParams,
        ResourceContents,
    },
};
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
        Ok(vec!["get_screenshot".to_owned(), "use_figma".to_owned()])
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
            ReadToolCall::Screenshot { .. } => Ok(UpstreamResult {
                raw: json!({
                    "content": [{
                        "type": "image",
                        "mimeType": "image/png",
                        "data": reference_png_base64()
                    }]
                }),
            }),
            _ => Err(DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "composite export must stay on the one-call fast path",
                false,
            )),
        }
    }
}

#[tokio::test]
async fn reference_png_is_acquired_once_and_delivered_as_a_binary_resource() -> anyhow::Result<()> {
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
            "outputs": ["referencePng"]
        }),
    )
    .await?;

    assert_eq!(acquired["collection"]["figmaToolCalls"], 2);
    assert_eq!(acquired["referencePng"]["mimeType"], "image/png");
    assert_eq!(
        acquired["referencePng"]["dataBase64"],
        reference_png_base64()
    );
    assert_eq!(acquired["cache"]["capabilities"]["referencePng"], true);
    // No tsx was requested/produced by this export, so no deliverable
    // marker should be attached — it must not claim a devup-ui-tsx exists
    // when only a reference PNG was exported.
    assert!(acquired.get("deliverable").is_none());
    let artifact_id = acquired["cache"]["artifactId"].as_str().unwrap();

    let delivered_result = call_result(
        &client,
        "devup_figma_export",
        json!({
            "artifactId": artifact_id,
            "outputs": ["referencePng"],
            "delivery": "resource"
        }),
    )
    .await?;
    let delivered = delivered_result
        .structured_content
        .as_ref()
        .expect("structured compatibility summary");
    assert!(delivered.get("referencePng").is_none());
    assert_eq!(delivered["resources"][0]["mimeType"], "image/png");
    let manifest_uri = delivered["resources"][0]["uri"].as_str().unwrap();
    let link = delivered_result
        .content
        .iter()
        .filter_map(ContentBlock::as_resource_link)
        .find(|link| link.uri == manifest_uri)
        .expect("native manifest resource link");
    assert_eq!(link.mime_type.as_deref(), Some("application/json"));
    assert_eq!(link.size, None);
    assert_eq!(
        link.meta
            .as_ref()
            .and_then(|meta| meta.0.get("payloadMimeType"))
            .and_then(Value::as_str),
        Some("image/png")
    );
    let wire = serde_json::to_value(&delivered_result)?;
    assert!(wire["content"].as_array().is_some_and(|content| {
        content
            .iter()
            .any(|block| block["type"] == "resource_link" && block["uri"] == manifest_uri)
    }));
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 2);

    client.cancel().await?;
    task.await??;
    Ok(())
}

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &str,
    arguments: Value,
) -> anyhow::Result<Value> {
    Ok(call_result(client, name, arguments)
        .await?
        .structured_content
        .expect("structured tool output"))
}

async fn call_result(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &str,
    arguments: Value,
) -> anyhow::Result<CallToolResult> {
    let arguments: Map<String, Value> = arguments.as_object().cloned().unwrap();
    let result = client
        .call_tool(CallToolRequestParams::new(name.to_owned()).with_arguments(arguments))
        .await?;
    anyhow::ensure!(
        result.is_error != Some(true),
        "{}",
        result
            .structured_content
            .as_ref()
            .expect("structured error")
    );
    Ok(result)
}

/// The design in raw form is behind a door marked debug.
///
/// `rawSnapshot` and `rawPayload` describe the design rather than the screen.
/// Nothing that implements a screen needs them, and left in the ordinary list
/// they were taken as a matter of course - the server's own instructions used
/// to say to take `rawSnapshot` every time, which on a measured screen spent
/// about eight bytes for every one of code. They stay reachable, because
/// deciding whether a screen that looks wrong is the generator's fault means
/// reading the design beside the code, and that is what `debug` is for.
#[tokio::test]
async fn the_collected_design_is_reachable_only_in_debug() -> anyhow::Result<()> {
    let upstream = Arc::new(FastFixtureUpstream::complete());
    let server = DevupServer::new(Services::new(Arc::new(ConnectedAuth), upstream));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let url = "https://www.figma.com/design/FileKey123/Fixture?node-id=1-2";

    for output in ["rawSnapshot", "rawPayload"] {
        let refused = call_result(
            &client,
            "devup_figma_export",
            json!({ "url": url, "outputs": [output], "scope": "node" }),
        )
        .await
        .expect_err("the design in raw form is not part of implementing a screen");
        let message = refused.to_string();
        assert!(
            message.contains("debug: true"),
            "the refusal has to say which door to open: {message}"
        );
        assert!(
            message.contains(output),
            "and which output it is about: {message}"
        );
    }

    // Asking for it deliberately still works, and still costs what it costs.
    let debugging = call(
        &client,
        "devup_figma_export",
        json!({ "url": url, "outputs": ["tsx", "rawSnapshot"], "scope": "node", "debug": true }),
    )
    .await?;
    assert!(debugging["rawSnapshot"]["nodes"].is_object());
    assert!(debugging["tsx"].as_str().is_some_and(|tsx| !tsx.is_empty()));

    client.cancel().await?;
    task.await??;
    Ok(())
}

/// A clean conversion should carry the code and the grade, and not the
/// paperwork underneath the grade.
///
/// `fidelity` and `completenessReport` are the drill-down under `quality`:
/// on a clean run the first reports 100% across six axes and the second six
/// empty arrays, so both only repeat what `quality` already said. Measured on
/// this fixture they were 797 bytes of the 1,911 every response carried.
/// They are still sent whenever they disagree with a clean grade, and
/// whenever the caller asks for diagnostics - which the test above covers.
#[tokio::test]
async fn a_clean_capture_reports_its_projection_shortfall_by_default() -> anyhow::Result<()> {
    let upstream = Arc::new(FastFixtureUpstream::complete());
    let server = DevupServer::new(Services::new(Arc::new(ConnectedAuth), upstream));
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
            "outputs": ["tsx"],
            "scope": "node"
        }),
    )
    .await?;

    assert_eq!(result["status"], "partial");
    assert!(
        result["projectionIssues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["code"] == "DEVUP_CODEGEN_LAYOUT_UNCOVERED")
    );
    assert_eq!(result["quality"]["projection"], "lossy");
    assert_eq!(result["quality"]["acquisition"], "complete");
    assert!(result["tsx"].as_str().unwrap().contains("$primary"));
    // `completenessReport` is keyed on the capture, which is clean here.
    assert!(
        result.get("completenessReport").is_none(),
        "a clean capture says nothing the grade has not already said"
    );
    // R1: a clean acquisition does not make unclassified layout loss exact.
    let fidelity = result
        .get("fidelity")
        .expect("a projection shortfall must be explained");
    assert_eq!(result["quality"]["projection"], "lossy");
    assert!(
        fidelity["variables"]["basisPoints"].as_u64() < Some(10_000)
            || fidelity["layout"]["basisPoints"].as_u64() < Some(10_000)
            || fidelity["nodes"]["basisPoints"].as_u64() < Some(10_000)
            || fidelity["text"]["basisPoints"].as_u64() < Some(10_000)
            || fidelity["typography"]["basisPoints"].as_u64() < Some(10_000)
            || fidelity["assets"]["basisPoints"].as_u64() < Some(10_000),
        "it is sent because an axis is short: {fidelity}"
    );

    client.cancel().await?;
    task.await??;
    Ok(())
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
            "outputs": ["tsx", "devupJson", "rawSnapshot", "rawPayload", "sourceMap", "assetManifest"],
            "debug": true,
            "scope": "node",
            "includeDiagnostics": true
        }),
    )
    .await?;

    assert_eq!(first["status"], "partial");
    assert!(
        first["projectionIssues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["code"] == "DEVUP_CODEGEN_LAYOUT_UNCOVERED")
    );
    // The raw payload is the whole collection the snapshot came out of: the
    // same nodes, plus the variables the token names are read from, and
    // never the reference PNG, which has an output of its own.
    assert_eq!(first["rawPayload"]["snapshot"], first["rawSnapshot"]);
    assert!(first["rawPayload"]["variables"].is_object());
    assert!(first["rawPayload"].get("referencePng").is_none());
    assert_eq!(first["collection"]["figmaToolCalls"], 1);
    assert_eq!(first["cache"]["cacheHit"], false);
    assert!(first["cache"]["artifactId"].as_str().is_some());
    assert!(first["tsx"].as_str().unwrap().contains("$primary"));
    assert_eq!(first["status"], "partial");
    assert!(
        first["projectionIssues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["code"] == "DEVUP_CODEGEN_LAYOUT_UNCOVERED")
    );
    assert_eq!(first["quality"]["projection"], "lossy");
    // This call asks for diagnostics, so the drill-down under `quality` is
    // exactly what it should get.
    assert!(first["fidelity"].is_object());
    assert!(first["completenessReport"].is_object());
    // The code is available for inspection but is not a final exact output.
    assert_eq!(first["deliverable"]["isFinal"], false);
    assert!(
        !first["deliverable"]["availableOutputs"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // These restate something already in the response whether diagnostics
    // were asked for or not: `imports` and `usedTokens` restate the tsx's own
    // import line and its `$token`s. Neither is sent any more.
    for restated in ["imports", "usedTokens", "componentImports"] {
        assert!(
            first.get(restated).is_none(),
            "{restated} restates something already in the response and must not be sent"
        );
    }
    assert!(first["devupJson"].as_str().unwrap().contains("\"primary\""));
    assert_eq!(first["rawSnapshot"]["roots"], json!(["1:2"]));
    assert_eq!(first["sourceMap"]["version"], 2);
    // Both pictures the generated code points at: the child drawn from its
    // own image fill, and the root's second fill, which the code paints as a
    // background. A container is a layout box rather than an asset, but the
    // picture on it still has to be listed or nothing could deliver it.
    assert_eq!(
        asset_ids(&first["assetManifest"]),
        vec!["1:2:fills:1", "1:3:fills:0"]
    );
    assert_eq!(
        asset_by_id(&first["assetManifest"], "1:3:fills:0")["status"],
        "available"
    );
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

    let resource = call(
        &client,
        "devup_figma_export",
        json!({
            "artifactId": artifact_id,
            "outputs": ["tsx"],
            "delivery": "resource"
        }),
    )
    .await?;
    assert!(resource.get("tsx").is_none());
    let manifest_uri = resource["resources"][0]["uri"].as_str().unwrap();
    let manifest = client
        .read_resource(ReadResourceRequestParams::new(manifest_uri))
        .await?;
    let ResourceContents::TextResourceContents { text, .. } = &manifest.contents[0] else {
        panic!("resource manifest must be text")
    };
    let manifest: Value = serde_json::from_str(text)?;
    assert_eq!(manifest["name"], "tsx");
    assert_eq!(manifest["mimeType"], "text/typescript");
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 1);

    // The same acquisition answers a tsx-only and a theme-only projection
    // without going back to Figma, which is what devup_figma_to_ui and
    // devup_figma_to_json used to be for before they were folded into this
    // one tool as `outputs`.
    let ui_wrapper = call(
        &client,
        "devup_figma_export",
        json!({"url": url, "outputs": ["tsx"]}),
    )
    .await?;
    let json_wrapper = call(
        &client,
        "devup_figma_export",
        json!({"url": url, "outputs": ["devupJson"], "scope": "node"}),
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
            "debug": true,
            "scope": "node",
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
            "scope": "node"
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

    let error = incompatible
        .map(common::tool_error)
        .expect("tool result must report failure");
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
            "assetRequests": [{"assetId":"1:3:fills:0","renderMode":"node","format":"png","scale":2}]
        }),
    )
    .await?;

    assert_eq!(result["status"], "partial");
    assert!(
        result["projectionIssues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["code"] == "DEVUP_CODEGEN_LAYOUT_UNCOVERED")
    );
    assert_eq!(result["collection"]["figmaToolCalls"], 2);
    let exported = asset_by_id(&result["assetManifest"], "1:3:fills:0");
    assert_eq!(exported["status"], "exported");
    assert_eq!(exported["dataBase64"], STANDARD.encode(b"synthetic-png"));
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 2);

    client.cancel().await?;
    task.await??;
    Ok(())
}

#[tokio::test]
async fn resource_asset_manifest_reconstructs_the_exact_independent_binary() -> anyhow::Result<()> {
    let upstream = Arc::new(FastFixtureUpstream::complete());
    let root = unique_temp_dir("asset-resource")?;
    let output_path = root.join("asset.png");
    let server = DevupServer::with_output_roots(
        Services::new(Arc::new(ConnectedAuth), upstream.clone()),
        vec![root.clone()],
    )?;
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let result = call_result(
        &client,
        "devup_figma_export",
        json!({
            "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-2",
            "outputs": ["assetManifest"],
            "delivery": "resource",
            "assetRequests": [{
                "assetId":"1:3:fills:0","renderMode":"node",
                "format":"png",
                "scale":2,
                "outputPath": output_path.to_string_lossy()
            }]
        }),
    )
    .await?;
    let structured = result
        .structured_content
        .as_ref()
        .expect("structured compatibility summary");
    let summaries = structured["resources"].as_array().unwrap();
    assert_eq!(
        result.structured_content.as_ref().unwrap()["exportJob"]["assets"][0]["status"],
        "exported"
    );
    assert_eq!(
        result.structured_content.as_ref().unwrap()["exportJob"]["assets"][0]["fileState"],
        "written"
    );
    assert_eq!(
        summaries.len(),
        3,
        "asset manifest, image-fill audit and one binary asset"
    );
    assert!(
        summaries
            .iter()
            .any(|r| r["name"] == "image-fill-audit.json")
    );
    let manifest_uri = summaries
        .iter()
        .find(|item| item["name"] == "asset-manifest.json")
        .and_then(|item| item["uri"].as_str())
        .unwrap();
    let manifest_bytes = read_resource_bytes(&client, manifest_uri).await?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes)?;
    let exported = asset_by_id(&manifest, "1:3:fills:0").clone();
    let resource = &exported["resource"];
    let asset_uri = resource["uri"].as_str().expect("asset resource URI");
    assert_eq!(resource["mimeType"], "image/png");
    assert_eq!(resource["byteLength"], b"synthetic-png".len());
    assert_eq!(
        resource["sha256"],
        "294ad7145322ec19f8250cca8480a933f1ce8c9e2ad1038e7ae8930d55a6598a"
    );
    assert!(exported.get("dataBase64").is_none());

    let resource_bytes = read_resource_bytes(&client, asset_uri).await?;
    assert_eq!(resource_bytes, b"synthetic-png");
    assert_eq!(fs::read(&output_path)?, resource_bytes);
    let asset_links = result
        .content
        .iter()
        .filter_map(ContentBlock::as_resource_link)
        .filter(|link| {
            link.meta
                .as_ref()
                .and_then(|meta| meta.0.get("payloadMimeType"))
                .and_then(Value::as_str)
                == Some("image/png")
        })
        .collect::<Vec<_>>();
    assert_eq!(asset_links.len(), 1);
    assert_eq!(asset_links[0].uri, asset_uri);
    assert_eq!(
        asset_links[0].mime_type.as_deref(),
        Some("application/json")
    );
    assert_eq!(asset_links[0].size, None);
    assert_eq!(
        asset_links[0]
            .meta
            .as_ref()
            .and_then(|meta| meta.0.get("payloadSha256"))
            .and_then(Value::as_str),
        Some("294ad7145322ec19f8250cca8480a933f1ce8c9e2ad1038e7ae8930d55a6598a")
    );
    let calls_after_first_export = upstream.calls.load(Ordering::SeqCst);

    let reused = call_result(
        &client,
        "devup_figma_export",
        json!({
            "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-2",
            "outputs": ["assetManifest"],
            "delivery": "resource",
            "assetRequests": [{
                "assetId":"1:3:fills:0","renderMode":"node",
                "format":"png",
                "scale":2,
                "outputPath": output_path.to_string_lossy()
            }]
        }),
    )
    .await?;
    let reused_structured = reused
        .structured_content
        .as_ref()
        .expect("reused structured compatibility summary");
    let reused_resources = reused_structured["resources"].as_array().unwrap();
    let reused_manifest_uri = reused_resources
        .iter()
        .find(|resource| resource["name"] == "asset-manifest.json")
        .and_then(|resource| resource["uri"].as_str())
        .expect("reused asset manifest URI");
    let reused_manifest: Value =
        serde_json::from_slice(&read_resource_bytes(&client, reused_manifest_uri).await?)?;
    let reused_asset_uri = asset_by_id(&reused_manifest, "1:3:fills:0")["resource"]["uri"]
        .as_str()
        .expect("reused asset resource URI");
    assert!(
        reused_resources
            .iter()
            .any(|resource| resource["uri"] == reused_asset_uri),
        "the reused manifest must reference the actually reused resource"
    );
    assert_eq!(
        read_resource_bytes(&client, reused_asset_uri).await?,
        resource_bytes
    );
    assert_eq!(
        upstream.calls.load(Ordering::SeqCst),
        calls_after_first_export,
        "resource projection reuse must not reacquire the Figma payload"
    );

    client.cancel().await?;
    task.await??;
    fs::remove_dir_all(root)?;
    Ok(())
}

async fn read_resource_bytes(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    manifest_uri: &str,
) -> anyhow::Result<Vec<u8>> {
    let manifest = client
        .read_resource(ReadResourceRequestParams::new(manifest_uri))
        .await?;
    let ResourceContents::TextResourceContents { text, .. } = &manifest.contents[0] else {
        anyhow::bail!("resource manifest must be JSON text")
    };
    let manifest: Value = serde_json::from_str(text)?;
    let mut bytes = Vec::new();
    for index in 0..manifest["chunkCount"].as_u64().unwrap() {
        let chunk_uri = format!(
            "devup://artifact/{}/outputs/{}/chunks/{index}",
            manifest["artifactId"].as_str().unwrap(),
            manifest["outputId"].as_str().unwrap()
        );
        let chunk = client
            .read_resource(ReadResourceRequestParams::new(chunk_uri))
            .await?;
        match &chunk.contents[0] {
            ResourceContents::TextResourceContents { text, .. } => {
                bytes.extend_from_slice(text.as_bytes());
            }
            ResourceContents::BlobResourceContents { blob, .. } => {
                bytes.extend_from_slice(&STANDARD.decode(blob.as_bytes())?);
            }
            _ => anyhow::bail!("unsupported resource content"),
        }
    }
    Ok(bytes)
}

/// One asset of a manifest, by the id it is known under. The manifest lists
/// every picture the generated code points at, so a test that wants a
/// particular one asks for it by name rather than by where it happens to sit.
fn asset_by_id<'a>(manifest: &'a Value, asset_id: &str) -> &'a Value {
    manifest["assets"]
        .as_array()
        .expect("manifest assets")
        .iter()
        .find(|asset| asset["assetId"] == asset_id)
        .unwrap_or_else(|| panic!("no asset {asset_id} in {:?}", asset_ids(manifest)))
}

fn asset_ids(manifest: &Value) -> Vec<String> {
    manifest["assets"]
        .as_array()
        .expect("manifest assets")
        .iter()
        .map(|asset| asset["assetId"].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn unique_temp_dir(label: &str) -> anyhow::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "devup-mcp-{label}-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    fs::create_dir_all(&path)?;
    Ok(path)
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
            "assetRequests": [{"assetId":"1:3:fills:0","renderMode":"node","format":"png","scale":2}]
        }),
    )
    .await?;
    let artifact_id = acquired["cache"]["artifactId"].as_str().unwrap();
    assert_eq!(acquired["cache"]["capabilities"]["assetCaptureCount"], 1);
    assert!(!serde_json::to_string(&acquired["cache"]["capabilities"])?.contains("1:3:fills:0"));
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 2);

    for request in [
        json!({"assetId":"1:3:fills:0","renderMode":"node","format":"svg","scale":2}),
        json!({"assetId":"1:3:fills:0","renderMode":"node","format":"png","scale":1}),
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
        let error = reused
            .map(common::tool_error)
            .expect("tool result must report failure");
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
                "debug": true,
                        "strict": true
                    })
                .as_object()
                .cloned()
                .unwrap(),
            ),
        )
        .await;

    let error = result
        .map(common::tool_error)
        .expect("tool result must report failure");
    assert!(
        error.to_string().contains("partial"),
        "unexpected strict error: {error}"
    );
    client.cancel().await?;
    task.await??;
    Ok(())
}