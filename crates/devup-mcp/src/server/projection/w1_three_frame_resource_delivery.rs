#[tokio::test]
async fn r8_three_frame_requested_outputs_and_complete_proof_measure_local_projection() {
    let data: CollectedPayload = serde_json::from_str(include_str!(
        "../../../../../fixtures/r2/wquw-119-payload.json"
    ))
    .unwrap();
    let mut op = operation(&["tsx", "rawSnapshot", "sourceMap"]);
    if let PendingOperation::Export { frame_ids, .. } = &mut op {
        *frame_ids = vec![
            "3997:46315".into(),
            "3997:46715".into(),
            "3997:46333".into(),
        ];
    }
    let began = std::time::Instant::now();
    let store = ArtifactStore::default();
    let key = ArtifactRequestKey::from_collection(&CollectionRequest::new(
        data.target.clone(),
        CollectionScope::Node,
    ));
    let artifact = store.insert(key, data).await.unwrap();
    let policy = OutputPolicy::from_roots(vec![std::env::temp_dir()]).unwrap();
    // R17 source FILL issues exceed the unchanged inline limit. Exercise the
    // retained-artifact recovery and read every output, rather than hiding loss.
    let error = complete_operation(
        op.clone(),
        &artifact.payload,
        "fixture",
        &artifact,
        &policy,
        &store,
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupFigmaResponseTooLarge);
    assert_eq!(error.details["recoveryState"], "available");
    assert_eq!(
        error.details["nextAction"]["arguments"]["artifactId"],
        artifact.artifact_id
    );
    assert_eq!(
        error.details["nextAction"]["arguments"]["delivery"],
        "resource"
    );
    if let PendingOperation::Export { delivery, .. } = &mut op {
        *delivery = DeliveryMode::Resource;
    }
    let mut result =
        complete_operation(op, &artifact.payload, "fixture", &artifact, &policy, &store)
            .await
            .unwrap();
    let mut outputs = BTreeMap::new();
    for manifest in store.output_manifests().await {
        let mut bytes = Vec::new();
        for index in 0..manifest.chunk_count {
            bytes.extend(
                store
                    .read_output_chunk(&artifact.artifact_id, &manifest.output_id, index)
                    .await
                    .unwrap(),
            );
        }
        assert_eq!(bytes.len(), manifest.raw_bytes);
        outputs.insert(manifest.name, String::from_utf8(bytes).unwrap());
    }
    // Fixed literal loss can reduce metadata below the unchanged spill limit.
    // Every requested frame body/map and the raw snapshot remain mandatory;
    // complete diagnostic proof follows the existing inline/resource policy.
    let details = outputs
        .get("projection-details.json")
        .map(|text| serde_json::from_str::<Value>(text).unwrap());
    let response_proof = details.as_ref().map_or(&result, |body| &body["response"]);
    assert!(response_proof["projectionEvidence"].is_array());
    assert!(response_proof["projectionIssues"].is_array());
    let proof_frames = details
        .as_ref()
        .map_or(&result["frames"], |body| &body["frames"]);
    assert_eq!(proof_frames.as_array().unwrap().len(), 3);
    for frame in proof_frames.as_array().unwrap() {
        assert!(frame["projectionEvidence"].is_array());
        assert!(frame["projectionIssues"].is_array());
        if details.is_none() {
            assert!(
                frame["metadataPreviewFields"]
                    .get("projectionEvidence")
                    .is_none()
            );
            assert!(
                frame["metadataPreviewFields"]
                    .get("projectionIssues")
                    .is_none()
            );
        }
    }
    assert!(outputs.contains_key("image-fill-audit.json"));
    for name in ["interaction-hints.json", "behavior-plan.json", "development-contract.json"] {
        let body: Value = serde_json::from_str(&outputs[name]).unwrap();
        assert!(body.is_object() || body.is_array(), "{name} must retain structured evidence");
    }
    let expected_retained_files = 3 * 2 + 5 + usize::from(details.is_some());
    assert_eq!(
        outputs.len(),
        expected_retained_files,
        "{:?}",
        outputs.keys()
    );
    assert_eq!(
        serde_json::from_str::<Value>(&outputs["raw-snapshot.json"]).unwrap(),
        serde_json::to_value(&artifact.payload.snapshot).unwrap()
    );
    for index in 0..3 {
        let tsx = &outputs[&format!("frame-{}.tsx", index + 1)];
        assert!(tsx.contains("export function"));
        result["frames"][index]["tsx"] = json!(tsx);
        result["frames"][index]["sourceMap"] =
            serde_json::from_str(&outputs[&format!("frame-{}.source-map.json", index + 1)])
                .unwrap();
    }
    assert_eq!(result["frames"].as_array().unwrap().len(), 3);
    for frame in result["frames"].as_array().unwrap() {
        assert!(frame["tsx"].is_string());
        assert!(frame["sourceMap"].is_object());
        assert!(frame["sourceMap"]["rasterizedAssets"].is_array());
    }
    eprintln!(
        "R8 offline 3-frame/9 requested projection units/{} retained files: {:?}; no upstream timing inferred",
        outputs.len(),
        began.elapsed()
    );
}
