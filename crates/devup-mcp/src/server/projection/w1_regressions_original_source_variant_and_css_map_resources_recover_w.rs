
#[tokio::test]
async fn original_source_variant_and_css_map_resources_recover_without_reacquiring() {
    let store = ArtifactStore::default();
    let data = original_source_payload();
    let artifact = store
        .insert(
            ArtifactRequestKey::from_collection(&CollectionRequest::new(
                data.target.clone(),
                CollectionScope::Node,
            )),
            data,
        )
        .await
        .unwrap();
    let mut op = operation(&["tsx", "assetManifest", "assetCssMap"]);
    if let PendingOperation::Export { delivery, .. } = &mut op {
        *delivery = DeliveryMode::Resource;
    }
    let value = complete_operation(
        op,
        &artifact.payload,
        "fixture",
        &artifact,
        &OutputPolicy::from_roots(vec![std::env::temp_dir()]).unwrap(),
        &store,
    )
    .await
    .unwrap();
    let manifests = store.output_manifests().await;
    assert!(manifests.iter().any(|m| m.name == "asset-css-map.json"));
    assert_eq!(
        manifests
            .iter()
            .filter(|m| m.mime_type == "image/webp")
            .count(),
        2
    );
    for manifest in manifests.iter().filter(|m| m.mime_type == "image/webp") {
        let mut bytes = Vec::new();
        for index in 0..manifest.chunk_count {
            bytes.extend(
                store
                    .read_resource_output(&artifact.artifact_id, &manifest.output_id, Some(index))
                    .await
                    .unwrap()
                    .1,
            );
        }
        assert_eq!(
            image::guess_format(&bytes).unwrap(),
            image::ImageFormat::WebP
        );
        assert_eq!(sha256_hex(&bytes), manifest.sha256);
    }
    assert_eq!(
        value["nextAction"]["arguments"]["artifactId"],
        artifact.artifact_id
    );
    assert!(value["assetCssMap"]["assets"].is_null());
}

fn p3_asset_payload(second_bytes: &[u8]) -> CollectedPayload {
    let mut data = payload();
    data.snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .fields
        .insert("childrenIds".into(), json!(["1:2", "1:3"]));
    for (id, bytes) in [("1:2", b"first image".as_slice()), ("1:3", second_bytes)] {
        data.snapshot.nodes.insert(
            id.into(),
            serde_json::from_value(json!({
                "id":id,"type":"RECTANGLE","fields":{"name":"Logo","parentId":"1:1",
                "visible":true,"width":40,"height":40,"x":0,"y":0,"isAsset":true,
                "fills":[{"type":"IMAGE","imageHash":id,"scaleMode":"FILL"}]}
            }))
            .unwrap(),
        );
        data.assets.push(serde_json::from_value(json!({
                "assetId":format!("{id}:fills:0"),"nodeId":id,"field":"fills/0","sourceKind":"image",
                "status":"exported","format":"png","scale":1,"mimeType":"image/png",
                "dataBase64":STANDARD.encode(bytes),"byteLength":bytes.len(),"sha256":sha256_hex(bytes)
            })).unwrap());
    }
    data
}

#[tokio::test]
async fn image_modes_keep_file_manifest_and_shared_resource_observations_consistent() {
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(8, 4)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let bytes = encoded.into_inner();
    for delivery_mode in [DeliveryMode::Inline, DeliveryMode::Resource] {
        let mut data = p3_asset_payload(&bytes);
        data.assets.clear();
        for (id, mode) in [
            ("1:2", devup_mcp_figma::AssetRenderMode::Node),
            ("1:3", devup_mcp_figma::AssetRenderMode::Fill),
        ] {
            data.assets.push(
                devup_mcp_figma::exported_asset_from_bytes(
                    &devup_mcp_figma::AssetRequest {
                        usage_aspect_ratio: None,
                        max_width: None,
                        srcset: false,
                        visible_scope_node_id: None,
                        asset_id: format!("{id}:fills:0"),
                        node_id: id.into(),
                        field: "fills/0".into(),
                        image_hash: Some(id.into()),
                        format: devup_mcp_figma::AssetFormat::Png,
                        scale: 1.0,
                        render_mode: mode,
                        quality: None,
                        lossless: None,
                    },
                    &bytes,
                )
                .unwrap(),
            );
        }
        let path = std::env::temp_dir().join(format!(
            "image-modes-{}-{:?}.png",
            std::process::id(),
            delivery_mode
        ));
        let mut op = operation(&["assetManifest"]);
        if let PendingOperation::Export {
            delivery,
            asset_output_paths,
            ..
        } = &mut op
        {
            *delivery = delivery_mode;
            asset_output_paths.insert("1:2:fills:0".into(), path.to_string_lossy().into_owned());
        }
        let store = ArtifactStore::default();
        let artifact = store
            .insert(
                ArtifactRequestKey::from_collection(&CollectionRequest::new(
                    data.target.clone(),
                    CollectionScope::Node,
                )),
                data,
            )
            .await
            .unwrap();
        let result = complete_operation(
            op,
            &artifact.payload,
            "fixture",
            &artifact,
            &OutputPolicy::from_roots(vec![std::env::temp_dir()]).unwrap(),
            &store,
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let manifest = if delivery_mode == DeliveryMode::Resource {
            let manifest = store
                .output_manifests()
                .await
                .into_iter()
                .find(|m| m.name == "asset-manifest.json")
                .unwrap();
            let mut bytes = Vec::new();
            for index in 0..manifest.chunk_count {
                bytes.extend(
                    store
                        .read_resource_output(
                            &artifact.artifact_id,
                            &manifest.output_id,
                            Some(index),
                        )
                        .await
                        .unwrap()
                        .1,
                );
            }
            serde_json::from_slice::<Value>(&bytes).unwrap()
        } else {
            result["assetManifest"].clone()
        };
        let assets = manifest["assets"].as_array().unwrap();
        for (asset, mode) in assets.iter().zip(["node", "fill"]) {
            assert_eq!(asset["renderMode"], mode);
            assert_eq!(asset["renderInfo"]["outputSize"]["width"], 8.0);
            assert_eq!(asset["renderInfo"]["aspectRatioComparison"], "mismatch");
            assert_eq!(
                asset["outputPath"],
                result["outputPaths"][format!("asset:{}", asset["assetId"].as_str().unwrap())]
            );
            assert!(asset["suggestedFilename"].as_str().unwrap().contains(mode));
        }
        if delivery_mode == DeliveryMode::Resource {
            let resource = result["resources"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["mimeType"] == "image/png")
                .unwrap();
            let metadata = resource["assetMetadata"]["assets"].as_array().unwrap();
            assert_eq!(metadata.len(), 2);
            for (alias, asset) in metadata.iter().zip(assets) {
                assert_eq!(alias["renderMode"], asset["renderMode"]);
                assert_eq!(alias["renderInfo"], asset["renderInfo"]);
                assert_eq!(alias["suggestedFilename"], asset["suggestedFilename"]);
            }
        }
        std::fs::remove_file(path).unwrap();
    }
}

#[tokio::test]
async fn original_fill_artifacts_cannot_silently_replace_rendered_codegen_assets() {
    let mut data = p3_asset_payload(b"same image");
    data.assets[0].render_mode = devup_mcp_figma::AssetRenderMode::Fill;
    for output in ["tsx", "componentTsx", "responsiveTsx"] {
        let error = project(data.clone(), operation(&[output]))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::DevupInvalidInput);
        assert!(error.message.contains("renderMode node"));
    }
}

#[tokio::test]
async fn auto_native_assets_reproject_code_and_audit_without_changing_cached_capture() {
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(40, 40)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let bytes = encoded.into_inner();
    let mut data = p3_asset_payload(&bytes);
    for asset in &mut data.assets {
        asset.render_mode = devup_mcp_figma::AssetRenderMode::Auto;
        asset.source_kind = "image-fill".into();
        asset.data_base64 = Some(STANDARD.encode(&bytes));
        asset.byte_length = Some(bytes.len());
        asset.sha256 = Some(sha256_hex(&bytes));
        asset.render_info = Some(
            json!({"representation":"native-node","sourceProperties":{"scaleMode":"CROP","imageTransform":[[0.5,0,0],[0,1,0]],"effects":[{"type":"LAYER_BLUR","radius":12}]},"autoDecision":{"eligible":false,"selected":"node","reasons":["paint-transform-or-unknown"]}}),
        );
    }
    let store = ArtifactStore::default();
    let artifact = store
        .insert(
            ArtifactRequestKey::from_collection(&CollectionRequest::new(
                data.target.clone(),
                CollectionScope::Node,
            )),
            data,
        )
        .await
        .unwrap();
    let before = artifact.payload.assets.clone();
    let policy = OutputPolicy::from_roots(vec![std::env::temp_dir()]).unwrap();
    for outputs in [
        &["tsx", "componentTsx", "assetManifest", "sourceMap"][..],
        &["assetManifest"][..],
        &["tsx"][..],
    ] {
        let result = complete_operation(
            operation(outputs),
            &artifact.payload,
            "fixture",
            &artifact,
            &policy,
            &store,
        )
        .await
        .unwrap();
        assert_eq!(result["imageFillAudit"]["total"], 2);
        assert_eq!(result["imageFillAudit"]["exact"], 0);
        assert_eq!(result["imageFillAudit"]["unknown"], 2);
        if outputs.contains(&"assetManifest") {
            for asset in result["assetManifest"]["assets"].as_array().unwrap() {
                assert_eq!(asset["renderMode"], "auto");
                assert_eq!(asset["renderInfo"]["renderedAs"], "node-render");
                assert_eq!(asset["renderInfo"]["effectsBakedIntoAsset"], true);
                assert_eq!(asset["renderInfo"]["canvasFidelity"], "unverified");
            }
        }
        if outputs.contains(&"sourceMap") {
            let baked = result["sourceMap"]["rasterizedAssets"].as_array().unwrap();
            assert_eq!(baked.len(), 2);
            assert!(
                baked.iter().all(
                    |a| a["effectsBakedIntoAsset"] == true && a["cssEffectsSuppressed"] == true
                )
            );
        }
    }
    assert_eq!(artifact.payload.assets, before);
}

fn webp_test_payload(count: usize) -> CollectedPayload {
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(40, 40)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let mut data = p3_asset_payload(png.get_ref());
    data.assets.clear();
    data.snapshot.nodes.get_mut("1:1").unwrap().fields.insert(
        "childrenIds".into(),
        json!((2..count + 2).map(|i| format!("1:{i}")).collect::<Vec<_>>()),
    );
    let prototype = data.snapshot.nodes["1:2"].clone();
    for i in 2..count + 2 {
        let id = format!("1:{i}");
        let mut node = prototype.clone();
        node.id = id.clone();
        data.snapshot.nodes.insert(id.clone(), node);
        let request = devup_mcp_figma::AssetRequest {
            usage_aspect_ratio: None,
            max_width: None,
            srcset: false,
            visible_scope_node_id: None,
            asset_id: format!("{id}:fills:0"),
            node_id: id.clone(),
            field: "fills/0".into(),
            image_hash: Some(id),
            format: devup_mcp_figma::AssetFormat::Webp,
            scale: 1.0,
            render_mode: devup_mcp_figma::AssetRenderMode::Node,
            quality: Some(43),
            lossless: Some(false),
        };
        data.assets
            .push(devup_mcp_figma::exported_asset_from_bytes(&request, png.get_ref()).unwrap());
    }
    data
}

#[tokio::test]
async fn webp_image_audit_writes_a_supported_output_and_requires_design_capability() {
    let root = std::env::temp_dir().join(format!("devup-webp-audit-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("image-audit.json");
    let mut op = operation(&["imageAudit"]);
    if let PendingOperation::Export { output_paths, .. } = &mut op {
        output_paths.insert("imageAudit".into(), path.display().to_string());
    }
    let result = project(webp_test_payload(2), op).await.unwrap();
    let written: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(written["assets"].as_array().unwrap().len(), 2);
    assert!(
        result["outputPathResults"]["supportedKeys"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o == "imageAudit")
    );
    assert!(
        result["outputPathResults"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let row = &written["assets"][0];
    assert_eq!(row["nodeSize"]["width"], 40.0);
    assert_eq!(row["outputSize"]["width"], 40.0);
    assert_eq!(row["recommendedFormat"], "webp");
    assert_eq!(row["visibleSourceRatio"]["known"], false);
    assert_eq!(
        row["visibleSourceRatio"]["reason"],
        "baked-render-source-coverage-unverified"
    );
    assert!(
        row["visibleSourceRatioEvidence"]
            .as_str()
            .unwrap()
            .contains("not-visible-percentages")
    );
    let data = webp_test_payload(2);
    let mut request = CollectionRequest::new(data.target.clone(), CollectionScope::File);
    request.variables_only = true;
    let store = ArtifactStore::default();
    let artifact = store
        .insert(ArtifactRequestKey::from_collection(&request), data)
        .await
        .unwrap();
    assert!(
        super::super::validation::validate_artifact_projection(
            &artifact,
            &["imageAudit".into()],
            "file",
            &[]
        )
        .is_err()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn webp_preset_replays_more_than_six_captures_with_public_files_and_coupled_resources() {
    let root = std::env::temp_dir().join(format!("devup-webp-preset-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&root).unwrap();
    let root = crate::test_paths::canonical(&root);
    let data = webp_test_payload(9);
    let store = ArtifactStore::default();
    let mut request = CollectionRequest::new(data.target.clone(), CollectionScope::Node);
    request.web_ready_images = true;
    let artifact = store
        .insert(ArtifactRequestKey::from_collection(&request), data)
        .await
        .unwrap();
    assert_eq!(artifact.capabilities.asset_capture_count, 9);
    assert!(artifact.capabilities.web_ready_images);
    for mode in [DeliveryMode::Inline, DeliveryMode::Resource] {
        let mut op = operation(&["tsx", "assetManifest", "imageAudit"]);
        if let PendingOperation::Export {
            web_ready_images,
            asset_public_root,
            delivery,
            ..
        } = &mut op
        {
            *web_ready_images = true;
            *asset_public_root = Some(root.clone());
            *delivery = mode;
        }
        let result = complete_operation(
            op,
            &artifact.payload,
            "fixture",
            &artifact,
            &OutputPolicy::from_roots(vec![root.clone()]).unwrap(),
            &store,
        )
        .await
        .unwrap();
        assert_eq!(result["webReadyImages"]["total"], 9);
        assert_eq!(result["webReadyImages"]["failed"], 0);
        let files = result["outputPaths"].as_object().unwrap();
        assert_eq!(files.len(), 9);
        for path in files.values().filter_map(Value::as_str) {
            assert!(path.ends_with(".webp"));
            assert_eq!(
                image::guess_format(&std::fs::read(path).unwrap()).unwrap(),
                image::ImageFormat::WebP
            );
        }
        if mode == DeliveryMode::Inline {
            assert!(result["tsx"].as_str().unwrap().contains(".webp"));
            assert_eq!(result["imageAudit"]["assets"].as_array().unwrap().len(), 9);
            for a in result["assetManifest"]["assets"].as_array().unwrap() {
                assert_eq!(a["quality"], 43);
                assert!(a["path"].as_str().unwrap().ends_with(".webp"));
            }
        } else {
            assert!(result["imageAudit"]["assets"].is_null());
            assert!(result["webReadyImages"]["plan"].is_null());
            let args = &result["nextAction"]["arguments"];
            assert_eq!(args["preset"], "web-ready-images");
            assert!(args["assetRequests"].is_null());
            assert!(
                args["outputs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|o| o == "assetManifest")
            );
            let input: super::super::tools::FigmaExportInput =
                serde_json::from_value(args.clone()).unwrap();
            super::super::validation::parse_asset_requests(&input.asset_requests).unwrap();
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn webp_scaffold_chooses_webp_paths_before_mapping_and_preserves_alias_identity() {
    let root = std::env::temp_dir().join(format!("devup-webp-scaffold-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&root).unwrap();
    for write in [false, true] {
        let mut op = operation(&["tsx", "assetManifest", "pageScaffold"]);
        if let PendingOperation::Export {
            page_scaffold,
            output_paths,
            ..
        } = &mut op
        {
            *page_scaffold =
                Some(serde_json::from_value(json!({"write":write,"route":"/"})).unwrap());
            output_paths.insert("pageScaffold".into(), root.display().to_string());
        }
        let result = project(webp_test_payload(2), op).await.unwrap();
        assert!(result["tsx"].as_str().unwrap().contains(".webp"));
        for asset in result["assetManifest"]["assets"].as_array().unwrap() {
            assert!(asset["path"].as_str().unwrap().ends_with(".webp"));
            assert_eq!(asset["quality"], 43);
            if write {
                let bytes = std::fs::read(asset["outputPath"].as_str().unwrap()).unwrap();
                assert_eq!(asset["sha256"], sha256_hex(&bytes));
            }
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn webp_failed_capture_preserves_options_and_returns_partial_beside_success() {
    let mut data = webp_test_payload(2);
    let a = &mut data.assets[1];
    a.status = AssetStatus::Failed;
    a.data_base64 = None;
    a.byte_length = None;
    a.sha256 = None;
    a.mime_type = None;
    a.error_code = Some("DEVUP_ASSET_WEBP_POSTPROCESS_FAILED".into());
    let selections = data
        .assets
        .iter()
        .map(|a| devup_mcp_figma::AssetSelection {
            usage_aspect_ratio: None,
            max_width: None,
            srcset: false,
            visible_scope_node_id: None,
            asset_id: a.asset_id.clone(),
            format: a.format.unwrap(),
            scale: a.scale.unwrap(),
            render_mode: a.render_mode,
            quality: a.quality,
            lossless: a.lossless,
        })
        .collect();
    let mut op = operation(&["assetManifest"]);
    if let PendingOperation::Export { asset_captures, .. } = &mut op {
        *asset_captures = selections;
    }
    let result = project(data, op).await.unwrap();
    assert_eq!(result["status"], "partial");
    assert_eq!(result["assetManifest"]["assets"][1]["quality"], 43);
    assert_eq!(result["assetManifest"]["assets"][1]["status"], "failed");
}

#[tokio::test]
async fn large_image_fill_audit_preserves_all_rows_in_resource_with_bounded_inline_summary() {
    let data = payload();
    let store = ArtifactStore::default();
    let artifact = store
        .insert(
            ArtifactRequestKey::from_collection(&CollectionRequest::new(
                data.target.clone(),
                CollectionScope::Node,
            )),
            data,
        )
        .await
        .unwrap();
    let rows = (0..10000).map(|i| json!({"assetId":format!("synthetic-{i}:fills:0"),"nodeId":format!("synthetic-{i}"),"canvasFidelity":"unverified","reviewReasons":["image-transform"],"unknownReasons":["canvas-reference-not-compared","ancestor-dependencies-unknown"]})).collect::<Vec<_>>();
    let mut result = json!({"status":"partial","imageFillAudit":{"total":rows.len(),"exact":0,"needsReview":rows.len(),"unknown":rows.len(),"summary":"10000 fills require review","assets":rows},
            "imageAudit":{"total":rows.len(),"assets":rows}});
    assert!(serde_json::to_vec(&result).unwrap().len() > 1024 * 1024);
    let outputs = projected_outputs_from_result(result.as_object().unwrap()).unwrap();
    let attachment = apply_delivery(
        &mut result,
        DeliveryMode::Auto,
        &store,
        &artifact,
        outputs,
        None,
    )
    .await
    .unwrap();
    commit_delivery(attachment);
    assert_eq!(result["imageFillAudit"]["total"], 10000);
    assert!(result["imageAudit"]["assets"].is_null());
    assert!(
        result["imageAudit"]["detailsResource"]["uri"]
            .as_str()
            .unwrap()
            .starts_with("devup://")
    );
    assert!(result["imageFillAudit"]["assets"].is_null());
    assert!(
        result["imageFillAudit"]["detailsResource"]["uri"]
            .as_str()
            .unwrap()
            .starts_with("devup://")
    );
    assert!(serde_json::to_vec(&result).unwrap().len() < 16384);
    let resource = store
        .output_manifests()
        .await
        .into_iter()
        .find(|m| m.name == "image-fill-audit.json")
        .unwrap();
    let mut bytes = Vec::new();
    for index in 0..resource.chunk_count {
        bytes.extend(
            store
                .read_resource_output(&artifact.artifact_id, &resource.output_id, Some(index))
                .await
                .unwrap()
                .1,
        );
    }
    let full: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(full["assets"].as_array().unwrap().len(), 10000);
    assert_eq!(full["assets"][9999]["assetId"], "synthetic-9999:fills:0");
}

#[tokio::test]
async fn p3_public_mapping_and_dedup_rewrite_inline_and_written_tsx() {
    let directory = std::env::temp_dir().join(format!(
        "devup-p3-assets-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let root = crate::test_paths::canonical(&directory);
    let first_path = root.join("icons/BI icon.png");
    let second_path = root.join("icons/other.png");
    let tsx_path = root.join("Screen.tsx");
    let mut op = operation(&["tsx", "assetManifest"]);
    if let PendingOperation::Export {
        asset_public_root,
        asset_output_paths,
        output_paths,
        ..
    } = &mut op
    {
        *asset_public_root = Some(root.clone());
        asset_output_paths.insert("1:2:fills:0".into(), first_path.to_string_lossy().into());
        asset_output_paths.insert("1:3:fills:0".into(), second_path.to_string_lossy().into());
        output_paths.insert("tsx".into(), tsx_path.to_string_lossy().into());
    }
    let result = project(p3_asset_payload(b"first image"), op).await.unwrap();
    let code = std::fs::read_to_string(&tsx_path).unwrap();
    assert!(code.contains("/icons/BI%20icon.png"), "{code}");
    assert_eq!(std::fs::read_to_string(&tsx_path).unwrap(), code);
    assert_eq!(std::fs::read(&first_path).unwrap(), b"first image");
    assert!(!second_path.exists());
    for asset in result["assetManifest"]["assets"].as_array().unwrap() {
        assert_eq!(asset["path"], "/icons/BI%20icon.png");
        assert_eq!(
            std::path::Path::new(asset["outputPath"].as_str().unwrap()),
            first_path
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn p3_same_name_different_bytes_keep_distinct_paths_and_default_names() {
    let result = project(
        p3_asset_payload(b"different image"),
        operation(&["tsx", "assetManifest"]),
    )
    .await
    .unwrap();
    let assets = result["assetManifest"]["assets"].as_array().unwrap();
    assert_ne!(assets[0]["path"], assets[1]["path"]);
    for asset in assets {
        assert!(
            result["tsx"]
                .as_str()
                .unwrap()
                .contains(asset["path"].as_str().unwrap())
        );
    }
}

#[tokio::test]
async fn p3_identical_assets_share_one_binary_resource() {
    let mut op = operation(&["tsx", "assetManifest"]);
    if let PendingOperation::Export { delivery, .. } = &mut op {
        *delivery = DeliveryMode::Resource;
    }
    let result = project(p3_asset_payload(b"first image"), op).await.unwrap();
    let resources = result["resources"].as_array().unwrap();
    assert_eq!(
        resources
            .iter()
            .filter(|resource| resource["mimeType"] == "image/png")
            .count(),
        1
    );
}

#[test]
fn p3_rewrites_all_code_variants_without_cascading_or_prefix_changes() {
    let paths = BTreeMap::from([
        ("/icons/a.svg".into(), "/icons/b.svg".into()),
        ("/icons/b.svg".into(), "/icons/c.svg".into()),
    ]);
    let mut result = json!({"tsx":"<Image src=\"/icons/a.svg\" />",
            "componentTsx":"<Box maskImage=\"url(/icons/a.svg)\" />",
            "responsiveTsx":"<Image src=\"/icons/b.svg\" />",
            "frames":[{"tsx":"<Image src=\"/icons/a.svg\" />"}],
            "other":"/icons/a.svg"});
    rewrite_result_asset_references(&mut result, &paths);
    assert_eq!(result["tsx"], "<Image src=\"/icons/b.svg\" />");
    assert_eq!(result["frames"][0]["tsx"], result["tsx"]);
    assert_eq!(
        result["componentTsx"],
        "<Box maskImage=\"url(/icons/b.svg)\" />"
    );
    assert_eq!(result["responsiveTsx"], "<Image src=\"/icons/c.svg\" />");
    assert_eq!(result["other"], "/icons/a.svg");
    assert_eq!(
        rewrite_asset_references("url(/icons/a.svg.extra)", &paths),
        "url(/icons/a.svg.extra)"
    );
}

#[tokio::test]
async fn p3_selection_reference_png_followup_targets_single_url() {
    let result = project(section(2), operation(&["tsx", "referencePng"]))
        .await
        .unwrap();
    let args = &result["nextAction"]["example"]["arguments"];
    assert!(args["url"].is_string());
    assert!(args.get("frameIds").is_none());
    assert!(args.get("artifactId").is_none());
    assert_eq!(args["scope"], "node");
}

#[tokio::test]
async fn p3_default_paths_are_not_rewritten_to_requested_files() {
    let directory = std::env::temp_dir().join(format!(
        "devup-p3-default-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("chosen.png");
    let mut op = operation(&["tsx", "assetManifest"]);
    if let PendingOperation::Export {
        asset_output_paths, ..
    } = &mut op
    {
        asset_output_paths.insert("1:2:fills:0".into(), path.to_string_lossy().into());
    }
    let result = project(p3_asset_payload(b"different"), op).await.unwrap();
    assert!(path.exists());
    let asset = &result["assetManifest"]["assets"][0];
    assert_ne!(asset["path"], "/chosen.png");
    assert!(
        result["tsx"]
            .as_str()
            .unwrap()
            .contains(asset["path"].as_str().unwrap())
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn p3_unverified_equal_hashes_do_not_merge_different_bytes() {
    let mut data = p3_asset_payload(b"different");
    data.assets[1].sha256 = data.assets[0].sha256.clone();
    let error = project(data, operation(&["tsx", "assetManifest"]))
        .await
        .unwrap_err();
    assert!(error.message.contains("hash does not match"), "{error}");
    assert!(!super::super::validation::is_caller_mistake(&error));
}

#[tokio::test]
async fn p3_tsx_only_reprojection_preserves_deduplicated_references() {
    let data = p3_asset_payload(b"first image");
    let with_manifest = project(data.clone(), operation(&["tsx", "assetManifest"]))
        .await
        .unwrap();
    let without_manifest = project(data, operation(&["tsx"])).await.unwrap();
    assert_eq!(with_manifest["tsx"], without_manifest["tsx"]);
    assert!(without_manifest.get("assetManifest").is_none());
}

#[tokio::test]
async fn p3_canonical_reference_is_independent_of_requested_duplicate_file() {
    let directory = std::env::temp_dir().join(format!(
        "devup-p3-canonical-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let data = p3_asset_payload(b"first image");
    let canonical = project(data.clone(), operation(&["tsx", "assetManifest"]))
        .await
        .unwrap();
    let mut op = operation(&["tsx", "assetManifest"]);
    if let PendingOperation::Export {
        asset_output_paths, ..
    } = &mut op
    {
        asset_output_paths.insert(
            "1:3:fills:0".into(),
            directory.join("chosen.png").to_string_lossy().into(),
        );
    }
    let written = project(data, op).await.unwrap();
    assert_eq!(canonical["tsx"], written["tsx"]);
    assert_eq!(
        canonical["assetManifest"]["assets"][0]["path"],
        written["assetManifest"]["assets"][0]["path"]
    );
    std::fs::remove_dir_all(directory).unwrap();
}

fn section(count: usize) -> CollectedPayload {
    let mut payload = payload();
    let template = payload.snapshot.nodes["1:1"].clone();
    payload.target.node_id = Some("0:1".into());
    payload.snapshot.nodes.clear();
    payload.snapshot.roots.clear();
    let mut candidates = Vec::new();
    for i in 1..=count {
        let id = format!("1:{i}");
        let mut node = template.clone();
        node.id = id.clone();
        payload.snapshot.nodes.insert(id.clone(), node);
        payload.snapshot.roots.push(id.clone());
        candidates.push(json!({"nodeId":id,"name":"Screen","nodeType":"FRAME",
                "textPreview":"","visible":true,"bounds":{"x":0,"y":0,"width":390,"height":800},
                "parentId":"0:1","breadcrumb":[],"directChildCount":0,
                "subtreeNodeCount":1,"estimatedSerializedBytes":500,"selectionReasons":["inside-section"],
                "canonicalUrl":format!("https://www.figma.com/design/W1Fixture/Test?node-id=1-{i}")}));
    }
    payload.metadata = json!({"sectionIndex":{"fileKey":"W1Fixture","sourceVersion":null,
            "section":{"nodeId":"0:1","name":"Section","bounds":{"x":0,"y":0,"width":2000,"height":2000}},
            "candidates":candidates,"truncated":false}});
    payload
}

fn operation(outputs: &[&str]) -> PendingOperation {
    PendingOperation::Export {
        outputs: outputs.iter().map(|s| (*s).into()).collect(),
        component_name: None,
        include_diagnostics: true,
        root_layout: Default::default(),
        asset_names_per_node: true,
        scope: "node".into(),
        theme_options: Default::default(),
        strict: false,
        output_paths: BTreeMap::new(),
        page_scaffold: None,
        previous_design_fingerprints: None,
        frame_ids: vec![],
        all_screens: false,
        asset_captures: vec![],
        web_ready_images: false,
        asset_output_paths: BTreeMap::new(),
        asset_public_root: None,
        fields: vec![],
        response_selection: Default::default(),
        delivery: DeliveryMode::Inline,
        width_policy: None,
    }
}

fn section_without_index(count: usize, projection_truncated: bool) -> CollectedPayload {
    let mut data = section(count);
    data.metadata = json!({});
    let mut root = data.snapshot.nodes["1:1"].clone();
    root.id = "0:1".into();
    root.node_type = "SECTION".into();
    root.fields = serde_json::from_value(json!({
        "name":"Section", "visible":true, "x":0, "y":0,
        "width":2000, "height":2000,
        "childrenIds":data.snapshot.roots,
        "projectionTruncated":projection_truncated
    }))
    .unwrap();
    for node in data.snapshot.nodes.values_mut() {
        node.fields.insert("parentId".into(), json!("0:1"));
    }
    data.snapshot.nodes.insert(root.id.clone(), root);
    data.snapshot.roots = vec!["0:1".into()];
    data
}

#[tokio::test]
async fn r17_large_section_provides_bounded_continuation_batches() {
    let result = project(
        section_without_index(23, false),
        operation(&["tsx", "sourceMap", "rawSnapshot"]),
    )
    .await
    .unwrap();
    let next = &result["nextAction"];
    assert_eq!(next["maxFramesPerCall"], 4);
    let batches = next["batches"].as_array().expect("bounded batch examples");
    assert_eq!(batches.len(), 6);
    assert_eq!(
        batches
            .iter()
            .map(|b| b["arguments"]["frameIds"].as_array().unwrap().len())
            .sum::<usize>(),
        23
    );
    for batch in batches {
        assert!(batch["arguments"]["frameIds"].as_array().unwrap().len() <= 4);
    }
    assert!(
        !next["how"]
            .as_str()
            .unwrap()
            .contains("Use allScreens:true")
    );
}

#[tokio::test]
async fn r17_section_png_batches_use_single_direct_frame_urls() {
    let result = project(
        section_without_index(23, false),
        operation(&["tsx", "referencePng"]),
    )
    .await
    .unwrap();
    assert_eq!(result["nextAction"]["maxFramesPerCall"], 1);
    let batches = result["nextAction"]["batches"].as_array().unwrap();
    assert_eq!(batches.len(), 23);
    let mut urls = std::collections::BTreeSet::new();
    for batch in batches {
        let args = &batch["arguments"];
        assert!(args.get("artifactId").is_none());
        assert!(args.get("frameIds").is_none());
        assert_eq!(args["scope"], "node");
        assert_eq!(args["outputs"], json!(["tsx", "referencePng"]));
        let url = args["url"].as_str().unwrap();
        assert!(url.contains("node-id="));
        assert!(urls.insert(url));
    }
}

#[tokio::test]
async fn w1_selection_exactly_100_candidates_is_complete() {
    let result = project(section_without_index(100, false), operation(&["tsx"]))
        .await
        .unwrap();
    assert_eq!(result["status"], "selection_required");
    assert_eq!(result["selection"]["count"], 100);
    assert_eq!(result["selection"]["truncated"], false);
    assert_eq!(result["selection"]["status"], "complete");
}

#[tokio::test]
async fn w1_selection_over_100_candidates_is_partial() {
    let result = project(section_without_index(101, false), operation(&["tsx"]))
        .await
        .unwrap();
    assert_eq!(result["selection"]["count"], 100);
    assert_eq!(result["selection"]["truncated"], true);
    assert_eq!(result["selection"]["status"], "partial");
}

#[tokio::test]
async fn w1_selection_separates_projection_and_candidate_truncation() {
    let data = section_without_index(100, true);
    let explored =
        explore_snapshot(&data.snapshot, &data.target, &ExploreOptions { limit: 100 }).unwrap();
    assert!(explored.truncated);
    assert!(!explored.candidates_truncated);
    let result = project(data, operation(&["tsx"])).await.unwrap();
    assert_eq!(result["selection"]["truncated"], false);
    assert_eq!(result["selection"]["status"], "complete");
}
