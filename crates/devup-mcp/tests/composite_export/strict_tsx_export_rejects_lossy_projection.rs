

#[tokio::test]
async fn strict_tsx_export_rejects_lossy_projection() -> anyhow::Result<()> {
    let upstream = Arc::new(FastFixtureUpstream::lossy());
    // Path validation now precedes collection: use an allowed path so this
    // test still isolates strict projection rejection and no-write behavior.
    let server = DevupServer::with_output_roots(
        Services::new(Arc::new(ConnectedAuth), upstream),
        vec![std::env::temp_dir()],
    )?;
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
                    "strict": true,
                    "outputPaths": {"tsx": output_path.to_string_lossy()}
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
        "kind": "devupFastSnapshotEnvelope",
        "schemaVersion": 1,
        "source": {"fileKey": "FileKey123", "rootId": "1:2"},
        "snapshot": {
            "fileKey": "FileKey123",
            "version": "v1",
            "rootIds": ["1:2"],
            "nodes": [
                {
                    "id": "1:2",
                    "type": "FRAME",
                    "fields": {
                        "name": "Synthetic",
                        "childrenIds": ["1:3"],
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
                },
                {
                    "id": "1:3",
                    "type": "RECTANGLE",
                    "fields": {
                        "name": "Synthetic asset",
                        "parentId": "1:2",
                        "isAsset": true,
                        "fills": [{"type":"IMAGE","imageHash":"image-hash-123","scaleMode":"FILL"}]
                    },
                    "extra": {},
                    "fieldErrors": {}
                }
            ],
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
            "nodeCount": 2,
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
    let _ = envelope_bytes;

    // No binary transport exists any more: fast snapshots are always plain
    // text (`devupFastSnapshotEnvelope`). Omitting the cursor marker node is
    // treated by the decoder as a single, already-complete page.
    UpstreamResult {
        raw: json!({"content": [
            {"type": "text", "text": envelope.to_string()}
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
        "imageHash":request.image_hash,"format":request.format,"scale":request.scale,"renderMode":request.render_mode,
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

fn reference_png_base64() -> &'static str {
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="
}

#[tokio::test]
async fn feedback_37_outline_assets_auto_manifest_and_first_reply_selection() -> anyhow::Result<()> {
    let root = unique_temp_dir("feedback-37")?;
    let server = DevupServer::with_output_roots(Services::new(Arc::new(ConnectedAuth), Arc::new(FastFixtureUpstream::complete())), vec![root.clone()])?;
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move { server.serve(server_transport).await?.waiting().await?; anyhow::Ok(()) });
    let client = ().serve(client_transport).await?;
    let result = call_result(&client, "devup_figma_export", json!({
        "url":"https://www.figma.com/design/FileKey123/Fixture?node-id=1-2",
        "outputs":["outline"],"responseLimit":1,
        "responseFields":["exportJob.state","exportJob.assets"],
        "assetRequests":[{"assetId":"1:3:fills:0","renderMode":"node","format":"png","scale":2,"outputPath":root.join("icon.png")}]
    })).await?;
    let value = result.structured_content.as_ref().unwrap();
    assert_eq!(value["exportJob"]["state"], "complete");
    assert_eq!(value["selected"]["selections"][0]["values"], json!(["complete"]));
    assert!(value["warnings"].as_array().unwrap().iter().any(|warning| warning["code"] == "DEVUP_ASSET_MANIFEST_ADDED"));
    assert!(!value["reviewChecklist"].as_array().into_iter().flatten().any(|item| item["code"] == "ASSETS_INCOMPLETE" && item["severity"] == "required"), "{} {}", value["exportJob"]["assetProgress"], value["quality"]);
    assert_eq!(fs::read(root.join("icon.png"))?, b"synthetic-png");
    client.cancel().await?;
    task.await??;
    fs::remove_dir_all(root)?;
    Ok(())
}

#[tokio::test]
async fn p3_public_root_mapping_survives_resource_delivery_and_file_writes() -> anyhow::Result<()> {
    let upstream = Arc::new(FastFixtureUpstream::complete());
    let root = unique_temp_dir("p3-public-root")?;
    let output_path = root.join("icons").join("BI icon.png");
    let code_path = root.join("Screen.tsx");
    let server = DevupServer::with_output_roots(
        Services::new(Arc::new(ConnectedAuth), upstream),
        vec![root.clone()],
    )?;
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let result = call_result(&client, "devup_figma_export", json!({
        "url":"https://www.figma.com/design/FileKey123/Fixture?node-id=1-2",
        "outputs":["tsx","assetManifest"], "delivery":"resource", "assetPublicRoot":root,
        "outputPaths":{"tsx":code_path},
        "assetRequests":[{"assetId":"1:3:fills:0","renderMode":"node","format":"png","scale":2,"outputPath":output_path}]
    })).await?;
    let resources = result.structured_content.as_ref().unwrap()["resources"]
        .as_array()
        .unwrap();
    let manifest_uri = resources
        .iter()
        .find(|item| item["name"] == "asset-manifest.json")
        .unwrap()["uri"]
        .as_str()
        .unwrap();
    let manifest: Value =
        serde_json::from_slice(&read_resource_bytes(&client, manifest_uri).await?)?;
    assert_eq!(
        asset_by_id(&manifest, "1:3:fills:0")["path"],
        "/icons/BI%20icon.png"
    );
    let code_uri = resources.iter().find(|item| item["name"] == "tsx").unwrap()["uri"]
        .as_str()
        .unwrap();
    let code = String::from_utf8(read_resource_bytes(&client, code_uri).await?)?;
    assert!(code.contains("/icons/BI%20icon.png"), "{code}");
    assert_eq!(fs::read_to_string(&code_path)?, code);
    assert_eq!(fs::read(&output_path)?, b"synthetic-png");
    client.cancel().await?;
    task.await??;
    fs::remove_dir_all(root)?;
    Ok(())
}

#[derive(Default)]
struct R3InterruptedUpstream {
    timeout_first: bool,
    snapshots: AtomicUsize,
    first_asset: AtomicUsize,
    second_asset: AtomicUsize,
}
#[async_trait]
impl FigmaUpstream for R3InterruptedUpstream {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".into()])
    }
    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        match call {
            ReadToolCall::Snapshot { .. } => {
                self.snapshots.fetch_add(1, Ordering::SeqCst);
                Ok(fast_envelope_result(false, false))
            }
            ReadToolCall::AssetExport {
                version, request, ..
            } => {
                let counter = if request.asset_id == "1:2:fills:1" {
                    &self.first_asset
                } else {
                    &self.second_asset
                };
                let attempt = counter.fetch_add(1, Ordering::SeqCst);
                if request.asset_id == "1:3:fills:0" && attempt == 0 {
                    if self.timeout_first {
                        std::future::pending::<()>().await;
                    }
                    return Err(DevupError::new(
                        ErrorCode::DevupFigmaDirectUnavailable,
                        "synthetic interrupted asset read",
                        true,
                    ));
                }
                Ok(asset_export_result(version.as_deref(), &request))
            }
            _ => unreachable!(),
        }
    }
}

#[tokio::test]
async fn r3_asset_job_resumes_only_pending_read_and_recovers_identical_request()
-> anyhow::Result<()> {
    r3_assert_resume(false).await
}

#[tokio::test(start_paused = true)]
async fn r3_asset_job_timeout_keeps_first_asset_and_resumes_pending_read() -> anyhow::Result<()> {
    r3_assert_resume(true).await
}

async fn r3_assert_resume(timeout_first: bool) -> anyhow::Result<()> {
    let upstream = Arc::new(R3InterruptedUpstream {
        timeout_first,
        ..Default::default()
    });
    let server = DevupServer::new(Services::new(Arc::new(ConnectedAuth), upstream.clone()));
    let (st, ct) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(st).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(ct).await?;
    let args = json!({"url":"https://www.figma.com/design/FileKey123/Fixture?node-id=1-2", "outputs":["assetManifest"],
        "assetRequests":[{"assetId":"1:2:fills:1","renderMode":"node"},{"assetId":"1:3:fills:0","renderMode":"node"}]});
    let mut paused = call(&client, "devup_figma_export", args.clone()).await?;
    if timeout_first {
        assert_eq!(paused["assetJob"]["state"], "running");
        let timeout_ms = paused["exportJob"]["calls"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["timeoutMs"]
            .as_u64()
            .unwrap();
        assert!(timeout_ms >= paused["exportJob"]["assetTimeoutMs"].as_u64().unwrap());
        tokio::time::advance(std::time::Duration::from_millis(timeout_ms + 1000)).await;
        paused = call(
            &client,
            "devup_figma_export",
            json!({"jobId":paused["assetJob"]["jobId"]}),
        )
        .await?;
    }
    assert_eq!(paused["assetJob"]["state"], "paused", "{paused}");
    let id = paused["assetJob"]["jobId"].as_str().unwrap();
    assert_eq!(paused["exportJob"]["assets"][0]["status"], "exported");
    assert_eq!(
        paused["exportJob"]["assets"][0]["fileState"],
        "not-requested"
    );
    // Polls answer compact rows, and the alias no longer repeats the job.
    assert!(paused["exportJob"]["assets"][0].get("renderInfo").is_none());
    assert_eq!(paused["assetJob"]["aliasOf"], "exportJob");
    // responseFields on a poll answers only what it selected.
    let picked = call(
        &client,
        "devup_figma_export",
        json!({"jobId":id,"responseFields":["exportJob.assets.renderInfo"]}),
    )
    .await?;
    assert_eq!(picked["selections"][0]["matched"], 1, "{picked}");
    assert!(picked["selections"][0]["values"][0].is_object());
    assert_eq!(picked["exportJob"]["state"], "paused");
    assert!(picked.get("assetJob").is_none(), "{picked}");
    let recovered = call(&client, "devup_figma_export", args).await?;
    assert_eq!(recovered["assetJob"]["jobId"], id);
    let resumed = call(
        &client,
        "devup_figma_export",
        json!({"jobId":id,"jobAction":"resume"}),
    )
    .await?;
    let mut result = resumed;
    for _ in 0..50 {
        if result["assetJob"]["state"] == "complete" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        result = call(&client, "devup_figma_export", json!({"jobId":id})).await?;
    }
    assert_eq!(result["assetJob"]["state"], "complete", "{result}");
    assert_eq!(
        asset_by_id(&result["assetManifest"], "1:3:fills:0")["status"],
        "exported"
    );
    assert_eq!(upstream.snapshots.load(Ordering::SeqCst), 1);
    assert_eq!(upstream.first_asset.load(Ordering::SeqCst), 1);
    assert_eq!(upstream.second_asset.load(Ordering::SeqCst), 2);
    client.cancel().await?;
    task.await??;
    Ok(())
}

#[tokio::test]
async fn r3_oversized_assets_rejected_before_upstream() -> anyhow::Result<()> {
    let upstream = Arc::new(FastFixtureUpstream::complete());
    let server = DevupServer::new(Services::new(Arc::new(ConnectedAuth), upstream.clone()));
    let (st, ct) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(st).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(ct).await?;
    let assets: Vec<_> = (0..16)
        .map(|i| json!({"assetId":format!("1:{i}:node")}))
        .collect();
    let result=client.call_tool(CallToolRequestParams::new("devup_figma_export").with_arguments(json!({
        "url":"https://www.figma.com/design/FileKey123/Fixture?node-id=1-2","outputs":["assetManifest"],"assetRequests":assets
    }).as_object().unwrap().clone())).await;
    common::tool_error(result?);
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 0);
    client.cancel().await?;
    task.await??;
    Ok(())
}

#[derive(Default)]
struct R3BatchUpstream {
    snapshots: AtomicUsize,
    assets: AtomicUsize,
}
#[async_trait]
impl FigmaUpstream for R3BatchUpstream {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".into()])
    }
    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        match call {
            ReadToolCall::Snapshot { .. } => {
                self.snapshots.fetch_add(1, Ordering::SeqCst);
                let raw = fast_envelope_result(false, false).raw;
                let mut envelope: Value =
                    serde_json::from_str(raw["content"][0]["text"].as_str().unwrap()).unwrap();
                let mut nodes = vec![envelope["snapshot"]["nodes"][0].clone()];
                nodes[0]["fields"]["childrenIds"] =
                    json!((10..26).map(|i| format!("1:{i}")).collect::<Vec<_>>());
                nodes[0]["fields"]["fills"]
                    .as_array_mut()
                    .unwrap()
                    .truncate(1);
                for i in 10..26 {
                    let mut node = envelope["snapshot"]["nodes"][1].clone();
                    node["id"] = json!(format!("1:{i}"));
                    nodes.push(node);
                }
                envelope["snapshot"]["nodes"] = json!(nodes);
                envelope["integrity"]["nodeCount"] = json!(17);
                loop {
                    let size = serde_json::to_vec(&envelope).unwrap().len();
                    if envelope["integrity"]["utf8Bytes"] == size {
                        break;
                    }
                    envelope["integrity"]["utf8Bytes"] = json!(size);
                }
                Ok(UpstreamResult {
                    raw: json!({"content":[{"type":"text","text":envelope.to_string()}]}),
                })
            }
            ReadToolCall::AssetExport {
                version, request, ..
            } => {
                self.assets.fetch_add(1, Ordering::SeqCst);
                Ok(asset_export_result(version.as_deref(), &request))
            }
            _ => unreachable!(),
        }
    }
}

#[tokio::test(start_paused = true)]
async fn r3_sixteen_assets_succeed_in_recommended_batches() -> anyhow::Result<()> {
    let upstream = Arc::new(R3BatchUpstream::default());
    let server = DevupServer::new(Services::new(Arc::new(ConnectedAuth), upstream.clone()));
    let (st, ct) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(st).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(ct).await?;
    let assets: Vec<_> = (10..26)
        .map(|i| json!({"assetId":format!("1:{i}:fills:0"),"renderMode":"node"}))
        .collect();
    for batch in assets.chunks(3) {
        let mut result = call(
            &client,
            "devup_figma_export",
            json!({"url":"https://www.figma.com/design/FileKey123/Fixture?node-id=1-2",
            "outputs":["assetManifest"],"assetRequests":batch}),
        )
        .await?;
        for _ in 0..120 {
            if result["assetJob"]["state"] == "complete" {
                break;
            }
            tokio::time::advance(std::time::Duration::from_secs(1)).await;
            result = call(
                &client,
                "devup_figma_export",
                json!({"jobId":result["assetJob"]["jobId"]}),
            )
            .await?;
        }
        assert_eq!(result["assetJob"]["state"], "complete", "{result}");
        for asset in batch {
            assert_eq!(
                asset_by_id(&result["assetManifest"], asset["assetId"].as_str().unwrap())["status"],
                "exported"
            );
        }
    }
    assert_eq!(upstream.assets.load(Ordering::SeqCst), 16);
    assert_eq!(upstream.snapshots.load(Ordering::SeqCst), 6);
    client.cancel().await?;
    task.await??;
    Ok(())
}

#[tokio::test]
async fn r3_different_output_job_does_not_wait_on_unresumable_owner() -> anyhow::Result<()> {
    let upstream = Arc::new(R3InterruptedUpstream::default());
    let server = DevupServer::new(Services::new(Arc::new(ConnectedAuth), upstream));
    let (st, ct) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(st).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(ct).await?;
    let mut args = json!({"url":"https://www.figma.com/design/FileKey123/Fixture?node-id=1-2","outputs":["assetManifest"],
        "assetRequests":[{"assetId":"1:2:fills:1","renderMode":"node"},{"assetId":"1:3:fills:0","renderMode":"node"}]});
    let owner = call(&client, "devup_figma_export", args.clone()).await?;
    assert_eq!(owner["assetJob"]["state"], "paused");
    args["outputs"] = json!(["tsx", "assetManifest"]);
    let independent = call(&client, "devup_figma_export", args).await?;
    assert_eq!(
        independent["assetJob"]["state"], "complete",
        "{independent}"
    );
    assert_ne!(owner["assetJob"]["jobId"], independent["assetJob"]["jobId"]);
    let resumed = call(
        &client,
        "devup_figma_export",
        json!({"jobId":owner["assetJob"]["jobId"],"jobAction":"resume"}),
    )
    .await?;
    assert_eq!(resumed["assetJob"]["state"], "complete");
    client.cancel().await?;
    task.await??;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn r3_cancelled_client_recovers_retained_asset_job() -> anyhow::Result<()> {
    let upstream = Arc::new(R3InterruptedUpstream {
        timeout_first: true,
        ..Default::default()
    });
    let server = DevupServer::new(Services::new(Arc::new(ConnectedAuth), upstream.clone()));
    let (st, ct) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(st).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(ct).await?;
    let args = json!({"url":"https://www.figma.com/design/FileKey123/Fixture?node-id=1-2","outputs":["assetManifest"],
        "assetRequests":[{"assetId":"1:2:fills:1","renderMode":"node"},{"assetId":"1:3:fills:0","renderMode":"node"}]});
    let dropped = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        call(&client, "devup_figma_export", args.clone()),
    )
    .await;
    assert!(dropped.is_err());
    let recovered = call(&client, "devup_figma_export", args).await?;
    let id = &recovered["assetJob"]["jobId"];
    assert!(id.is_string());
    assert_eq!(upstream.snapshots.load(Ordering::SeqCst), 1);
    let timeout_ms = recovered["exportJob"]["calls"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["timeoutMs"]
        .as_u64()
        .unwrap();
    tokio::time::advance(std::time::Duration::from_millis(timeout_ms + 1000)).await;
    let paused = call(&client, "devup_figma_export", json!({"jobId":id})).await?;
    assert_eq!(paused["assetJob"]["state"], "paused");
    let complete = call(
        &client,
        "devup_figma_export",
        json!({"jobId":id,"jobAction":"resume"}),
    )
    .await?;
    assert_eq!(complete["assetJob"]["state"], "complete");
    assert_eq!(upstream.first_asset.load(Ordering::SeqCst), 1);
    assert_eq!(upstream.second_asset.load(Ordering::SeqCst), 2);
    client.cancel().await?;
    task.await??;
    Ok(())
}

#[tokio::test]
async fn r3_distinct_resource_jobs_keep_both_manifests_readable() -> anyhow::Result<()> {
    let upstream = Arc::new(R3InterruptedUpstream::default());
    let server = DevupServer::new(Services::new(Arc::new(ConnectedAuth), upstream));
    let (st, ct) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(st).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(ct).await?;
    let mut args = json!({"url":"https://www.figma.com/design/FileKey123/Fixture?node-id=1-2","outputs":["assetManifest"],"delivery":"resource",
        "assetRequests":[{"assetId":"1:2:fills:1","renderMode":"node"},{"assetId":"1:3:fills:0","renderMode":"node"}]});
    let owner = call(&client, "devup_figma_export", args.clone()).await?;
    assert_eq!(owner["assetJob"]["state"], "paused");
    args["outputs"] = json!(["tsx", "assetManifest"]);
    let second = call(&client, "devup_figma_export", args).await?;
    let first = call(
        &client,
        "devup_figma_export",
        json!({"jobId":owner["assetJob"]["jobId"],"jobAction":"resume"}),
    )
    .await?;
    for value in [first, second] {
        assert_eq!(value["assetJob"]["state"], "complete");
        assert!(
            value["exportJob"]["assets"]
                .as_array()
                .unwrap()
                .iter()
                .all(|a| a["status"] == "exported")
        );
        let uri = value["resources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == "asset-manifest.json")
            .unwrap()["uri"]
            .as_str()
            .unwrap();
        let bytes = read_resource_bytes(&client, uri).await?;
        let manifest: Value = serde_json::from_slice(&bytes)?;
        assert_eq!(asset_by_id(&manifest, "1:3:fills:0")["status"], "exported");
    }
    client.cancel().await?;
    task.await??;
    Ok(())
}

#[derive(Debug)]
struct R8SlowSnapshot;
#[async_trait]
impl FigmaUpstream for R8SlowSnapshot {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".into()])
    }
    async fn call_read_tool(&self, _: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        Ok(fast_envelope_result(false, false))
    }
}
#[tokio::test]
async fn r8_code_only_slow_capture_returns_pollable_job_before_client_deadline()
-> anyhow::Result<()> {
    let server = DevupServer::with_output_roots(
        Services::new(Arc::new(ConnectedAuth), Arc::new(R8SlowSnapshot)),
        vec![std::env::temp_dir()],
    )?;
    let (st, ct) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(st).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(ct).await?;
    let began = std::time::Instant::now();
    let response=call(&client,"devup_figma_export",json!({"url":"https://www.figma.com/design/FileKey123/Fixture?node-id=1-2","outputs":["tsx","rawSnapshot","sourceMap"],"debug":true,"delivery":"inline"})).await?;
    assert!(
        began.elapsed() < std::time::Duration::from_secs(2),
        "client blocked for {:?}",
        began.elapsed()
    );
    let id = response["exportJob"]["jobId"]
        .as_str()
        .expect("code-only collection needs a resumable job");
    assert!(response["exportJob"]["calls"][0]["detail"]["nodeId"].is_string());
    let mut result = response.clone();
    for _ in 0..8 {
        result = call(&client, "devup_figma_export", json!({"jobId":id})).await?;
        if result["exportJob"]["state"] == "complete" {
            break;
        }
    }
    assert_eq!(result["exportJob"]["state"], "complete");
    assert!(result["tsx"].is_string());
    assert!(
        result["exportJob"]["calls"][0]["elapsedMs"]
            .as_u64()
            .unwrap()
            >= 2900
    );
    client.cancel().await?;
    task.await??;
    Ok(())
}

#[tokio::test]
async fn r8_completed_exports_do_not_exhaust_active_job_capacity() -> anyhow::Result<()> {
    let server = DevupServer::with_output_roots(
        Services::new(
            Arc::new(ConnectedAuth),
            Arc::new(FastFixtureUpstream::complete()),
        ),
        vec![std::env::temp_dir()],
    )?;
    let (st, ct) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(st).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(ct).await?;
    for index in 0..10 {
        let result=call(&client,"devup_figma_export",json!({"url":"https://www.figma.com/design/FileKey123/Fixture?node-id=1-2","outputs":["tsx"],"componentName":format!("Screen{index}"),"delivery":"inline"})).await?;
        assert!(
            result["tsx"].is_string(),
            "completed export {index} must not consume active capacity: {result}"
        );
    }
    client.cancel().await?;
    task.await??;
    Ok(())
}
