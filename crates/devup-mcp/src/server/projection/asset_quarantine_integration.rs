#[tokio::test]
async fn naming_review_quarantines_every_write_then_named_reprojection_publishes_consistently() {
    let root =
        std::env::temp_dir().join(format!("devup-quarantine-flow-{}", rand::random::<u64>()));
    std::fs::create_dir_all(root.join("public")).unwrap();
    let policy = OutputPolicy::from_roots(vec![root.clone()]).unwrap();
    let mut data = payload();
    let node = data.snapshot.nodes.get_mut("1:1").unwrap();
    node.node_type = "VECTOR".into();
    node.fields.insert("name".into(), json!("Image 1"));
    let bytes = br#"<svg viewBox="0 0 40 40"><path d="M0 0L40 40"/></svg>"#;
    data.assets.push(
        devup_mcp_figma::exported_asset_from_bytes(
            &devup_mcp_figma::AssetRequest {
                usage_aspect_ratio: None,
                max_width: None,
                srcset: false,
                visible_scope_node_id: None,
                render_mode: devup_mcp_figma::AssetRenderMode::Node,
                quality: None,
                lossless: None,
                asset_id: "1:1:node".into(),
                node_id: "1:1".into(),
                field: "node".into(),
                image_hash: None,
                format: devup_mcp_figma::AssetFormat::Svg,
                scale: 1.0,
            },
            bytes,
        )
        .unwrap(),
    );
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
    let make = |filename: &str| {
        let mut op = operation(&["tsx", "assetManifest"]);
        if let PendingOperation::Export {
            asset_public_root,
            asset_output_paths,
            output_paths,
            ..
        } = &mut op
        {
            *asset_public_root = Some(root.join("public"));
            asset_output_paths.insert(
                "1:1:node".into(),
                root.join("public").join(filename).to_string_lossy().into(),
            );
            output_paths.insert(
                "tsx".into(),
                root.join("Generated.tsx").to_string_lossy().into(),
            );
        }
        op
    };
    let blocked = complete_operation(
        make("images/image.svg"),
        &artifact.payload,
        "fixture",
        &artifact,
        &policy,
        &store,
    )
    .await
    .unwrap();
    assert_eq!(blocked["status"], "review-required");
    assert_eq!(blocked["assetQuarantine"]["published"], false);
    assert_eq!(blocked["deliverable"]["codeDelivered"], false);
    assert!(blocked.get("tsx").is_none());
    assert!(!root.join("Generated.tsx").exists());
    assert!(!root.join("public/images/image.svg").exists());
    assert!(!root.join(".devup/public-assets.json").exists());
    assert_eq!(
        blocked["nextAction"]["arguments"]["artifactId"],
        artifact.artifact_id
    );
    let published = complete_operation(
        make("icons/brand/logo.svg"),
        &artifact.payload,
        "fixture",
        &artifact,
        &policy,
        &store,
    )
    .await
    .unwrap();
    assert!(published.get("assetQuarantine").is_none());
    assert!(
        std::fs::read_to_string(root.join("Generated.tsx"))
            .unwrap()
            .contains("/icons/brand/logo.svg")
    );
    assert_eq!(
        std::fs::read(root.join("public/icons/brand/logo.svg")).unwrap(),
        bytes
    );
    let registry: Value =
        serde_json::from_slice(&std::fs::read(root.join(".devup/public-assets.json")).unwrap())
            .unwrap();
    assert_eq!(registry["assets"][0]["path"], "/icons/brand/logo.svg");
    assert!(!root.join("public/images/image.svg").exists());
    drop(policy);
    std::fs::remove_dir_all(root).unwrap();
}
