#[tokio::test]
async fn hero_node_source_selection_preserves_the_requested_file_destination() {
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(8, 4)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let bytes = encoded.into_inner();
    let mut data = p3_asset_payload(&bytes);
    data.assets.truncate(1);
    let hero = data.snapshot.nodes.get_mut("1:2").unwrap();
    hero.node_type = "FRAME".into();
    hero.fields.insert("name".into(), json!("Hero"));
    hero.fields.insert("childrenIds".into(), json!(["heading"]));
    data.snapshot.nodes.insert("heading".into(), serde_json::from_value(json!({
        "id":"heading","type":"TEXT","fields":{"characters":"Hero heading"}
    })).unwrap());
    data.assets[0].data_base64 = Some(STANDARD.encode(&bytes));
    data.assets[0].byte_length = Some(bytes.len());
    data.assets[0].sha256 = Some(sha256_hex(&bytes));
    data.assets[0].render_mode = devup_mcp_figma::AssetRenderMode::OriginalSource;
    let path = std::env::temp_dir().join(format!("hero-original-source-{}.png", std::process::id()));
    let mut op = operation(&["assetManifest"]);
    if let PendingOperation::Export { asset_captures, asset_output_paths, .. } = &mut op {
        asset_captures.push(serde_json::from_value(json!({
            "assetId":"1:2:node","format":"png","scale":1
        })).unwrap());
        asset_output_paths.insert("1:2:node".into(), path.to_string_lossy().into_owned());
    }
    let store = ArtifactStore::default();
    let artifact = store.insert(
        ArtifactRequestKey::from_collection(&CollectionRequest::new(data.target.clone(), CollectionScope::Node)),
        data,
    ).await.unwrap();
    let result = complete_operation(
        op, &artifact.payload, "fixture", &artifact,
        &OutputPolicy::from_roots(vec![std::env::temp_dir()]).unwrap(), &store,
    ).await.unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let asset = &result["assetManifest"]["assets"][0];
    assert_eq!(asset["assetId"], "1:2:fills:0");
    assert_eq!(asset["renderMode"], "original-source");
    assert_eq!(asset["outputPath"], result["outputPaths"]["asset:1:2:fills:0"]);
    std::fs::remove_file(path).unwrap();
}
