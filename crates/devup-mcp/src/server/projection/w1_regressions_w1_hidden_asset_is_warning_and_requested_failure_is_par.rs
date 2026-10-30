
#[tokio::test]
async fn w1_hidden_asset_is_warning_and_requested_failure_is_partial() {
    let mut data = payload();
    data.assets.push(
        serde_json::from_value(json!({"assetId":"1:1:fills:0","nodeId":"1:1",
            "field":"fills/0","sourceKind":"image","status":"failed","format":"png","scale":1,
            "errorCode":"DEVUP_ASSET_NODE_HIDDEN"}))
        .unwrap(),
    );
    let mut op = operation(&["assetManifest"]);
    if let PendingOperation::Export { asset_captures, .. } = &mut op {
        asset_captures.push(devup_mcp_figma::AssetSelection {
            usage_aspect_ratio: None,
            max_width: None,
            srcset: false,
            visible_scope_node_id: None,
            render_mode: devup_mcp_figma::AssetRenderMode::Node,
            quality: None,
            lossless: None,
            asset_id: "1:1:fills:0".into(),
            format: devup_mcp_figma::AssetFormat::Png,
            scale: 1.0,
        });
    }
    let result = project(data, op).await.unwrap();
    assert_eq!(result["failures"], json!([]));
    assert_eq!(result["warnings"][0]["assetId"], "1:1:fills:0");
    assert_eq!(result["status"], "complete");
}

#[tokio::test]
async fn w1_manifest_preserves_layer_name_and_section_paths_match() {
    let mut data = section(1);
    data.snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .fields
        .insert("name".into(), json!("BI - 아이콘"));
    data.snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .fields
        .insert("childrenIds".into(), json!(["2:1"]));
    data.snapshot.nodes.insert(
        "2:1".into(),
        serde_json::from_value(json!({"id":"2:1","type":"VECTOR",
            "fields":{"name":"BI - 아이콘", "parentId":"1:1","width":24,"height":24,"visible":true,
                "fills":[{"type":"SOLID","color":{"r":0,"g":0,"b":0}}]}}))
        .unwrap(),
    );
    let mut op = operation(&["tsx", "assetManifest"]);
    if let PendingOperation::Export { all_screens, .. } = &mut op {
        *all_screens = true;
    }
    let result = project(data, op).await.unwrap();
    let asset = &result["assetManifest"]["assets"][0];
    assert_eq!(asset["layerName"], "BI - 아이콘");
    let path = asset["path"].as_str().unwrap();
    assert!(path.is_ascii());
    assert!(!path.contains(' '));
    assert!(result["frames"][0]["tsx"].as_str().unwrap().contains(path));
}

#[tokio::test]
async fn w1_missing_master_is_partial_with_component_id() {
    let mut data = section(1);
    data.snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .fields
        .insert("childrenIds".into(), json!(["2:1"]));
    data.snapshot.nodes.insert(
        "2:1".into(),
        serde_json::from_value(json!({"id":"2:1","type":"INSTANCE",
            "fields":{"name":"Button","parentId":"1:1","componentId":"9:9","visible":true}}))
        .unwrap(),
    );
    let mut op = operation(&["tsx", "componentTsx"]);
    if let PendingOperation::Export { all_screens, .. } = &mut op {
        *all_screens = true;
    }
    let result = project(data, op).await.unwrap();
    assert_eq!(result["status"], "partial");
    assert!(result["frames"][0]["tsx"].is_string());
    assert_eq!(result["failures"][0]["details"]["componentId"], "9:9");
    assert_eq!(result["failures"][0]["details"]["instanceNodeId"], "2:1");
}

#[tokio::test]
async fn w1_source_map_without_generator_is_actionable() {
    let error = project(payload(), operation(&["sourceMap"]))
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupInvalidInput);
    assert!(error.message.contains("tsx"));
}

#[tokio::test]
async fn r6_source_map_error_explains_artifact_outputs() {
    let error = project(payload(), operation(&["rawSnapshot", "sourceMap"]))
        .await
        .unwrap_err();
    assert!(
        error.message.contains("artifactId") && error.message.contains("same call"),
        "{}",
        error.message
    );
    assert!(error.message.contains(
        r#"{"artifactId":"<artifactId>","outputs":["tsx","rawSnapshot","sourceMap"],"debug":true}"#
    ));
}

#[tokio::test]
async fn r6_completion_vertical_loss_is_scoped_to_tsx() {
    let mut data = payload();
    data.snapshot = serde_json::from_str(include_str!(
        "../../../../devup-mcp-devup-ui/tests/fixtures/r6-completion.json"
    ))
    .unwrap();
    data.target.node_id = Some("3997:46333".into());
    let mut op = operation(&["tsx", "sourceMap"]);
    if let PendingOperation::Export { root_layout, .. } = &mut op {
        *root_layout = devup_mcp_devup_ui::codegen::RootLayout::Embedded;
    }
    let result = project(data, op).await.unwrap();
    assert_eq!(result["deliverable"]["isFinal"], false);
    assert!(
        result["projectionIssues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["nodeId"] == "3997:46333"
                && d["property"] == "layoutSizingVertical"
                && d["fidelityImpact"] == "lossy"
                && d["details"]["output"] == "tsx")
    );
}

fn responsive_payload() -> CollectedPayload {
    let mut data = section(2);
    for (id, name, width) in [("1:1", "mobile", 390), ("1:2", "desktop", 1440)] {
        let node = data.snapshot.nodes.get_mut(id).unwrap();
        node.fields.insert("name".into(), json!(name));
        node.fields.insert("width".into(), json!(width));
    }
    data
}

#[tokio::test]
async fn w1_responsive_does_not_merge_unselected_frames() {
    let mut op = operation(&["responsiveTsx"]);
    if let PendingOperation::Export { frame_ids, .. } = &mut op {
        *frame_ids = vec!["1:1".into()];
    }
    let result = project(responsive_payload(), op).await;
    assert!(
        result.is_err(),
        "one selected width cannot be a responsive merge"
    );
}

#[tokio::test]
async fn w1_responsive_success_has_code_resource_and_no_empty_frames() {
    let mut op = operation(&["responsiveTsx"]);
    if let PendingOperation::Export {
        all_screens,
        delivery,
        ..
    } = &mut op
    {
        *all_screens = true;
        *delivery = DeliveryMode::Resource;
    }
    let result = project(responsive_payload(), op).await.unwrap();
    assert_ne!(result["quality"]["projection"], "not-requested");
    assert!(
        result.get("frames").is_none(),
        "no per-frame outputs were requested"
    );
    assert!(
        result["resources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["name"] == "responsiveTsx")
    );
}

#[tokio::test]
async fn w1_empty_all_screens_is_actionable() {
    let mut data = section(1);
    data.metadata["sectionIndex"]["candidates"][0]["bounds"] =
        json!({"x":0,"y":0,"width":100,"height":100});
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export { all_screens, .. } = &mut op {
        *all_screens = true;
    }
    let error = project(data, op).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupInvalidInput);
    assert!(error.message.contains("frameIds"));
}

#[tokio::test]
async fn w1_partial_frames_provide_retry_arguments() {
    let mut data = section(2);
    data.snapshot.nodes.remove("1:2");
    data.snapshot.roots.retain(|id| id != "1:2");
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export { all_screens, .. } = &mut op {
        *all_screens = true;
    }
    let result = project(data, op).await.unwrap();
    assert_eq!(result["resume"]["arguments"]["frameIds"], json!(["1:2"]));
    assert_eq!(result["resume"]["arguments"]["outputs"], json!(["tsx"]));
    assert!(result["resume"]["arguments"]["url"].is_string());
}

#[tokio::test]
async fn w1_discovered_hidden_assets_are_warnings_not_failed_deliverables() {
    let mut data = payload();
    let node = data.snapshot.nodes.get_mut("1:1").unwrap();
    node.node_type = "VECTOR".into();
    node.fields.insert("visible".into(), json!(false));
    let result = project(data, operation(&["assetManifest"])).await.unwrap();
    assert_eq!(result["status"], "complete");
    assert_eq!(result["assetManifest"]["assets"], json!([]));
    assert_eq!(
        result["warnings"][0]["errorCode"],
        "DEVUP_ASSET_NODE_HIDDEN"
    );
}

#[tokio::test]
async fn w1_index_unavailable_output_explains_collection_step() {
    let store = ArtifactStore::default();
    let data = section(2);
    let key = ArtifactRequestKey::from_collection(&CollectionRequest::new(
        data.target.clone(),
        CollectionScope::Node,
    ));
    let artifact = store.insert(key, data).await.unwrap();
    let error = super::super::validation::validate_artifact_projection(
        &artifact,
        &["assetManifest".into()],
        "node",
        &[],
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupInvalidInput);
    assert!(error.message.contains("frameIds"));
}
#[test]
fn w1_slash_in_layer_name_matches_manifest_and_code() {
    let original = "/icons/한글 Foo/Bar-1-2.svg";
    let path = host_safe_asset_path(original);
    let code = with_host_safe_asset_paths(&format!("<Box maskImage=\"url('{original}')\" />"));
    assert!(path.is_ascii());
    assert!(!path.contains(' '));
    assert_eq!(path.matches('/').count(), 2);
    assert!(code.contains(&path));
}

#[tokio::test]
async fn w1_captured_main_component_id_is_reported_when_missing() {
    let mut data = section(1);
    data.snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .fields
        .insert("childrenIds".into(), json!(["2:1"]));
    data.snapshot.nodes.insert(
        "2:1".into(),
        serde_json::from_value(json!({"id":"2:1","type":"INSTANCE",
            "fields":{"name":"Button","parentId":"1:1","mainComponentId":"9:9","visible":true}}))
        .unwrap(),
    );
    let mut op = operation(&["componentTsx"]);
    if let PendingOperation::Export { all_screens, .. } = &mut op {
        *all_screens = true;
    }
    let result = project(data, op).await.unwrap();
    assert_eq!(result["status"], "partial");
    assert_eq!(result["failures"][0]["details"]["componentId"], "9:9");
}

#[tokio::test]
async fn w1_responsive_text_loss_is_partial_and_strict_rejects() {
    let mut data = responsive_payload();
    for (root, id, text) in [
        ("1:1", "2:1", "Mobile only"),
        ("1:2", "2:2", "Desktop only"),
    ] {
        data.snapshot
            .nodes
            .get_mut(root)
            .unwrap()
            .fields
            .insert("childrenIds".into(), json!([id]));
        data.snapshot.nodes.insert(id.into(), serde_json::from_value(json!({"id":id,"type":"TEXT",
                "fields":{"name":"Label","parentId":root,"characters":text,"visible":true,"fontSize":16}})).unwrap());
    }
    let mut op = operation(&["responsiveTsx"]);
    if let PendingOperation::Export { all_screens, .. } = &mut op {
        *all_screens = true;
    }
    let result = project(data.clone(), op.clone()).await.unwrap();
    assert!(
        result["responsiveUnrepresented"]
            .as_array()
            .is_some_and(|notes| !notes.is_empty())
    );
    assert_eq!(result["status"], "partial");
    assert_eq!(result["quality"]["projection"], "lossy");
    if let PendingOperation::Export { strict, .. } = &mut op {
        *strict = true;
    }
    assert!(project(data, op).await.is_err());
}

#[tokio::test]
async fn w1_responsive_slot_collision_is_actionable() {
    let mut data = responsive_payload();
    data.snapshot
        .nodes
        .get_mut("1:2")
        .unwrap()
        .fields
        .insert("width".into(), json!(400));
    let mut op = operation(&["responsiveTsx"]);
    if let PendingOperation::Export { all_screens, .. } = &mut op {
        *all_screens = true;
    }
    let error = project(data, op).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupInvalidInput);
    assert!(error.message.contains("breakpoint"));
}

#[tokio::test]
async fn w1_missing_collected_outputs_explain_how_to_recollect() {
    for output in ["devupJson"] {
        let error = project(payload(), operation(&[output])).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::DevupInvalidInput);
        assert!(error.message.contains(output));
        assert!(error.details["snapshotRootIds"].is_array());
    }
    let result = project(payload(), operation(&["referencePng"])).await.unwrap();
    assert_eq!(result["outputResults"]["referencePng"]["state"], "unavailable");
    assert_ne!(result["status"], "complete");
    assert!(result["artifactId"].is_string());
}
#[tokio::test]
async fn w1_optional_responsive_collision_keeps_requested_tsx() {
    let mut data = responsive_payload();
    data.snapshot
        .nodes
        .get_mut("1:2")
        .unwrap()
        .fields
        .insert("width".into(), json!(400));
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export { all_screens, .. } = &mut op {
        *all_screens = true;
    }
    let result = project(data, op).await.unwrap();
    assert_eq!(result["frames"].as_array().unwrap().len(), 2);
    assert!(result["frames"][0]["tsx"].is_string());
    assert!(result.get("responsiveTsx").is_none());
}

#[test]
fn w1_ascii_slugs_do_not_merge_distinct_variant_assets() {
    assert_ne!(
        host_safe_asset_path("/icons/Icons=a b.svg"),
        host_safe_asset_path("/icons/Icons=a-b.svg")
    );
    assert_ne!(
        host_safe_asset_path("/icons/Foo Bar.svg"),
        host_safe_asset_path("/icons/Foo-Bar.svg")
    );
}
#[tokio::test]
async fn w7_scaffold_assets_match_public_paths_and_preview_has_no_asset_writes() {
    let dir = std::env::temp_dir().join(format!("devup-w7-assets-{:016x}", rand::random::<u64>()));
    std::fs::create_dir_all(dir.join("public")).unwrap();
    let root = crate::test_paths::canonical(&dir);
    let mut op = operation(&["pageScaffold"]);
    if let PendingOperation::Export {
        page_scaffold,
        output_paths,
        asset_public_root,
        asset_output_paths,
        ..
    } = &mut op
    {
        *page_scaffold = Some(page_scaffold::PageScaffoldOptions {
            route: "/settings/profile".into(),
            write: false,
            overwrite: false,
        });
        output_paths.insert("pageScaffold".into(), root.to_string_lossy().into_owned());
        *asset_public_root = Some(root.join("public"));
        asset_output_paths.insert(
            "1:2:fills:0".into(),
            root.join("public/icons/actual.png")
                .to_string_lossy()
                .into_owned(),
        );
    }
    let preview = project(p3_asset_payload(b"first image"), op.clone())
        .await
        .unwrap();
    assert_eq!(std::fs::read_dir(root.join("public")).unwrap().count(), 0);
    assert!(!root.join("src").exists());
    assert!(preview.get("tsx").is_none());
    assert!(preview.get("assetManifest").is_none());
    let preview_files = preview["pageScaffold"]["files"].as_array().unwrap();
    assert_eq!(
        preview_files.len(),
        3,
        "identical captured bytes reuse one asset"
    );
    if let PendingOperation::Export {
        page_scaffold: Some(options),
        ..
    } = &mut op
    {
        options.write = true;
    }
    let written = project(p3_asset_payload(b"first image"), op).await.unwrap();
    let files = written["pageScaffold"]["files"].as_array().unwrap();
    assert_eq!(preview_files, files);
    let code = files
        .iter()
        .find(|f| f["path"].as_str().unwrap().contains("components"))
        .unwrap()["content"]
        .as_str()
        .unwrap();
    assert!(code.contains("/icons/actual.png"), "{code}");
    for file in files {
        let expected = if file["encoding"] == "base64" {
            STANDARD.decode(file["content"].as_str().unwrap()).unwrap()
        } else {
            file["content"].as_str().unwrap().as_bytes().to_vec()
        };
        assert_eq!(
            std::fs::read(file["path"].as_str().unwrap()).unwrap(),
            expected
        );
    }
    assert_eq!(
        std::fs::read(root.join("public/icons/actual.png")).unwrap(),
        b"first image"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn w7_frame_scaffold_uses_frame_output_key_and_rejects_multi_route_guess() {
    let dir = std::env::temp_dir().join(format!("devup-w7-frame-{:016x}", rand::random::<u64>()));
    std::fs::create_dir(&dir).unwrap();
    let mut op = operation(&["pageScaffold"]);
    if let PendingOperation::Export {
        page_scaffold,
        output_paths,
        frame_ids,
        ..
    } = &mut op
    {
        *page_scaffold = Some(page_scaffold::PageScaffoldOptions {
            route: "settings".into(),
            write: true,
            overwrite: false,
        });
        *frame_ids = vec!["1:1".into()];
        output_paths.insert(
            "frame:1:1:pageScaffold".into(),
            dir.to_string_lossy().into_owned(),
        );
        output_paths.insert(
            "pageScaffold".into(),
            dir.join("ignored").to_string_lossy().into_owned(),
        );
    }
    let value = project(section(2), op.clone()).await.unwrap();
    assert!(value.get("pageScaffold").is_none());
    assert!(value["frames"][0].get("tsx").is_none());
    assert_eq!(
        value["frames"][0]["pageScaffold"]["files"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        value["outputPathResults"]["supportedKeys"]
            .as_array()
            .unwrap()
            .contains(&json!("frame:1:1:pageScaffold"))
    );
    assert!(
        value["outputPathResults"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["key"] == "pageScaffold")
    );
    assert!(!dir.join("ignored").exists());
    if let PendingOperation::Export { frame_ids, .. } = &mut op {
        frame_ids.push("1:2".into());
    }
    assert!(
        project(section(2), op)
            .await
            .unwrap_err()
            .message
            .contains("one frameId")
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn w7_section_selection_keeps_one_route_and_a_usable_frame_key() {
    let mut op = operation(&["pageScaffold"]);
    if let PendingOperation::Export {
        page_scaffold,
        output_paths,
        ..
    } = &mut op
    {
        *page_scaffold = Some(page_scaffold::PageScaffoldOptions {
            route: "/settings".into(),
            write: true,
            overwrite: false,
        });
        output_paths.insert("pageScaffold".into(), "project".into());
    }
    let value = project(section(3), op).await.unwrap();
    assert_eq!(value["status"], "selection_required");
    assert_eq!(value["nextAction"]["maxFramesPerCall"], 1);
    assert!(
        value["nextAction"].get("batches").is_none(),
        "one supplied route cannot be applied to every screen"
    );
    let args = &value["nextAction"]["example"]["arguments"];
    assert_eq!(args["pageScaffold"]["route"], "/settings");
    assert_eq!(args["outputPaths"]["frame:1:1:pageScaffold"], "project");
    assert!(args["outputPaths"].get("pageScaffold").is_none());
}
