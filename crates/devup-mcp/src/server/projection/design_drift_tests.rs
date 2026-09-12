#[tokio::test]
async fn w10_sidecar_write_uses_guarded_transaction_and_reports_replacement() {
    let dir = std::env::temp_dir().join(format!("devup-w10-{:016x}", rand::random::<u64>()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("screen.fingerprints.json");
    std::fs::write(&path, b"old sidecar").unwrap();
    let mut op = operation(&["designFingerprints"]);
    if let PendingOperation::Export { output_paths, .. } = &mut op {
        output_paths.insert(
            "designFingerprints".into(),
            path.to_string_lossy().into_owned(),
        );
    }
    let value = project(payload(), op).await.unwrap();
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved, value["designFingerprints"]);
    assert_eq!(saved["overwritePolicy"], "replace-existing");
    assert_eq!(saved["kind"], "devup-design-fingerprints");
    assert_eq!(
        value["outputPaths"]["designFingerprints"],
        json!(crate::test_paths::canonical(&path))
    );
    assert!(
        value["outputPathResults"]["supportedKeys"]
            .as_array()
            .unwrap()
            .contains(&json!("designFingerprints"))
    );
    assert_eq!(
        value["outputPathResults"]["committed"]["designFingerprints"],
        value["outputPaths"]["designFingerprints"]
    );
    assert!(value.get("sourceMap").is_none());
    assert!(value.get("tsx").is_none());
    assert!(value.get("designChanges").is_none());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn w10_sidecar_outside_allowed_root_refuses_entire_transaction() {
    let dir = std::env::temp_dir().join(format!("devup-w10-guard-{:016x}", rand::random::<u64>()));
    std::fs::create_dir(&dir).unwrap();
    let outside = dir.with_extension("outside.json");
    let code = dir.join("screen.tsx");
    std::fs::write(&code, b"human code").unwrap();
    let store = ArtifactStore::default();
    let data = payload();
    let key = ArtifactRequestKey::from_collection(&CollectionRequest::new(
        data.target.clone(),
        CollectionScope::Node,
    ));
    let artifact = store.insert(key, data).await.unwrap();
    let policy = OutputPolicy::from_roots(vec![dir.clone()]).unwrap();
    let mut op = operation(&["tsx", "designFingerprints"]);
    if let PendingOperation::Export { output_paths, .. } = &mut op {
        output_paths.insert("tsx".into(), code.to_string_lossy().into_owned());
        output_paths.insert(
            "designFingerprints".into(),
            outside.to_string_lossy().into_owned(),
        );
    }
    let error = complete_operation(op, &artifact.payload, "fixture", &artifact, &policy, &store)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupCodegenFailed);
    assert!(error.message.contains("outside the allowed root"));
    assert_eq!(std::fs::read(&code).unwrap(), b"human code");
    assert!(!outside.exists());
    drop(policy);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn w10_unrequested_sidecar_is_absent_and_does_not_write() {
    let path = std::env::temp_dir().join(format!(
        "devup-w10-unrequested-{:016x}.json",
        rand::random::<u64>()
    ));
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export { output_paths, .. } = &mut op {
        output_paths.insert(
            "designFingerprints".into(),
            path.to_string_lossy().into_owned(),
        );
    }
    let value = project(payload(), op).await.unwrap();
    assert!(value.get("designFingerprints").is_none());
    assert!(value.get("designChanges").is_none());
    assert!(!path.exists());
}

#[tokio::test]
async fn w10_comparison_requires_previous_sidecar() {
    let error = project(payload(), operation(&["designChanges"]))
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupInvalidInput);
    assert_eq!(error.details["type"], "designFingerprintMalformed");
}

#[tokio::test]
async fn w10_sidecar_and_code_cannot_silently_share_a_path() {
    let dir = std::env::temp_dir().join(format!(
        "devup-w10-collision-{:016x}",
        rand::random::<u64>()
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("screen.tsx");
    std::fs::write(&path, b"human code").unwrap();
    let mut op = operation(&["tsx", "designFingerprints"]);
    if let PendingOperation::Export { output_paths, .. } = &mut op {
        for key in ["tsx", "designFingerprints"] {
            output_paths.insert(key.into(), path.to_string_lossy().into_owned());
        }
    }
    let error = project(payload(), op).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupInvalidInput);
    assert!(error.message.contains("collision"));
    assert_eq!(std::fs::read(&path).unwrap(), b"human code");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn w10_saved_sidecar_compares_later_export_without_extra_outputs() {
    let data = payload();
    let original = project(
        data.clone(),
        operation(&["tsx", "sourceMap", "designFingerprints"]),
    )
    .await
    .unwrap();
    assert_eq!(
        original["sourceMap"]["designFingerprints"],
        original["designFingerprints"]["designFingerprints"]
    );
    let input: super::super::tools::FigmaExportInput = serde_json::from_value(json!({
        "outputs":["designChanges"], "previousDesignFingerprints":original["designFingerprints"].to_string()
    })).unwrap();
    super::super::validation::validate_outputs(&input.outputs, false).unwrap();
    let mut op = operation(&["designChanges"]);
    if let PendingOperation::Export {
        previous_design_fingerprints,
        ..
    } = &mut op
    {
        *previous_design_fingerprints = input.previous_design_fingerprints;
    }
    let unchanged = project(data.clone(), op.clone()).await.unwrap();
    assert_eq!(unchanged["designChanges"]["unchanged"], json!(["1:1"]));
    let mut changed = data;
    changed
        .snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .fields
        .insert("width".into(), json!(123));
    let result = project(changed, op).await.unwrap();
    assert_eq!(result["designChanges"]["changed"], json!(["1:1"]));
    for group in ["unchanged", "added", "removed"] {
        assert_eq!(result["designChanges"][group], json!([]));
    }
    for unrequested in ["tsx", "sourceMap", "designFingerprints"] {
        assert!(result.get(unrequested).is_none());
    }
    assert!(
        result["designChanges"]["meaning"]
            .as_str()
            .unwrap()
            .contains("does not mean the rendered screen changed")
    );
}

#[tokio::test]
async fn w10_resource_comparison_preserves_the_previous_sidecar() {
    let original = project(payload(), operation(&["designFingerprints"]))
        .await
        .unwrap();
    let previous = original["designFingerprints"].to_string();
    let mut op = operation(&["designChanges"]);
    if let PendingOperation::Export {
        previous_design_fingerprints,
        delivery,
        ..
    } = &mut op
    {
        *previous_design_fingerprints = Some(previous.clone());
        *delivery = DeliveryMode::Resource;
    }
    let result = project(payload(), op).await.unwrap();
    assert!(result["resources"].is_array());
    assert!(result.get("designChanges").is_none());
    assert_eq!(
        result["nextAction"]["arguments"]["previousDesignFingerprints"],
        previous
    );
    assert_eq!(
        result["nextAction"]["arguments"]["outputs"],
        json!(["designChanges"])
    );
}
