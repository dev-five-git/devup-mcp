#[tokio::test]
async fn omitted_format_writes_vector_svg_and_photo_webp() {
    for (vector, extension) in [(true, "svg"), (false, "webp")] {
        let mut data = original_source_payload();
        let node = data.snapshot.nodes.get_mut("1:2").unwrap();
        if vector {
            node.node_type = "VECTOR".into();
            node.fields.insert("fills".into(), json!([{ "type":"SOLID", "color":{"r":0,"g":0,"b":0} }]));
        }
        let (selections, _) = crate::server::validation::parse_asset_requests(&[
            serde_json::from_value(json!({"nodeId":"1:2"})).unwrap()
        ]).unwrap();
        let request = devup_mcp_figma::resolve_asset_selections(&data.snapshot, &selections).unwrap().remove(0);
        let bytes = if vector { b"<svg/>".to_vec() } else { STANDARD.decode(data.assets[0].data_base64.as_ref().unwrap()).unwrap() };
        if vector {
            data.assets = vec![devup_mcp_figma::exported_asset_from_bytes(&request, &bytes).unwrap()];
        } else {
            data.assets[0].usage_aspect_ratio = None;
            data.assets[0].max_width = None;
            data.assets[0].srcset = false;
            data.assets[0].lossless = None;
        }
        let root = std::env::temp_dir().join(format!("default-format-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&root).unwrap();
        let mut op = operation(&["assetManifest"]);
        if let PendingOperation::Export { asset_captures, asset_output_paths, .. } = &mut op {
            *asset_captures = selections;
            asset_output_paths.insert("1:2:node".into(), root.join("asset.png").to_string_lossy().into_owned());
        }
        let result = project(data, op).await.unwrap();
        let output = result["assetManifest"]["assets"].as_array().unwrap().iter().find(|asset| asset["status"] == "exported").unwrap();
        let path = std::path::Path::new(output["outputPath"].as_str().unwrap());
        assert_eq!(path.extension().unwrap(), extension);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        std::fs::remove_dir_all(root).unwrap();
    }
}

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
