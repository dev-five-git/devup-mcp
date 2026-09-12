async fn captured_fingerprint(data: CollectedPayload) -> String {
    let result = project(data, operation(&["tsx", "sourceMap"]))
        .await
        .unwrap();
    result["sourceMap"]["designFingerprints"]["1:1"]
        .as_str()
        .expect("requested sourceMap must carry per-node design fingerprints")
        .to_owned()
}

#[tokio::test]
async fn fingerprint_identical_capture_is_stable() {
    assert_eq!(
        captured_fingerprint(payload()).await,
        captured_fingerprint(payload()).await
    );
}

#[tokio::test]
async fn fingerprint_read_field_change_is_detected() {
    let mut changed = payload();
    changed
        .snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .fields
        .insert("width".into(), json!(420));
    assert_ne!(
        captured_fingerprint(payload()).await,
        captured_fingerprint(changed).await
    );
}

#[tokio::test]
async fn fingerprint_unread_fields_and_capture_metadata_are_ignored() {
    let mut changed = payload();
    let node = changed.snapshot.nodes.get_mut("1:1").unwrap();
    node.fields
        .insert("irrelevantPluginData".into(), json!({"timestamp":123}));
    node.extra.insert("annotations".into(), json!("updated"));
    node.field_errors
        .insert("irrelevantPluginData".into(), "failed".into());
    changed.source_version = Some("later".into());
    assert_eq!(
        captured_fingerprint(payload()).await,
        captured_fingerprint(changed).await
    );
}

#[tokio::test]
async fn fingerprint_field_and_nested_map_order_are_ignored() {
    let mut first = payload();
    let node = first.snapshot.nodes.get_mut("1:1").unwrap();
    node.fields.insert(
        "fills".into(),
        serde_json::from_str(r#"[{"type":"SOLID","color":{"r":1,"g":0,"b":0}}]"#).unwrap(),
    );
    let mut second = first.clone();
    let node = second.snapshot.nodes.get_mut("1:1").unwrap();
    node.fields = node.fields.clone().into_iter().rev().collect();
    node.fields.insert(
        "fills".into(),
        serde_json::from_str(r#"[{"color":{"b":0,"g":0,"r":1},"type":"SOLID"}]"#).unwrap(),
    );
    assert_eq!(
        captured_fingerprint(first).await,
        captured_fingerprint(second).await
    );
}

#[tokio::test]
async fn fingerprint_absent_null_and_failed_reads_are_distinct() {
    let absent = payload();
    let mut null = absent.clone();
    null.snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .fields
        .insert("opacity".into(), Value::Null);
    let mut failed = absent.clone();
    failed
        .snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .field_errors
        .insert("opacity".into(), "read failed".into());
    let fingerprints = [
        captured_fingerprint(absent).await,
        captured_fingerprint(null).await,
        captured_fingerprint(failed).await,
    ];
    assert_eq!(
        fingerprints
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
}

#[tokio::test]
async fn fingerprint_numeric_spelling_is_ignored() {
    let mut changed = payload();
    let node = changed.snapshot.nodes.get_mut("1:1").unwrap();
    node.fields
        .insert("width".into(), serde_json::from_str("3.90e2").unwrap());
    node.fields.insert("x".into(), json!(-0.0));
    assert_eq!(
        captured_fingerprint(payload()).await,
        captured_fingerprint(changed).await
    );
}

#[tokio::test]
async fn fingerprint_derived_text_segments_are_detected() {
    let mut changed = payload();
    changed
        .snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .fields
        .insert(
            "styledTextSegments".into(),
            json!([{"characters":"new", "fontSize":20}]),
        );
    assert_ne!(
        captured_fingerprint(payload()).await,
        captured_fingerprint(changed).await
    );
}

#[tokio::test]
async fn fingerprint_is_only_returned_when_source_map_is_requested() {
    let result = project(payload(), operation(&["tsx"])).await.unwrap();
    assert!(result.get("sourceMap").is_none());
    assert!(result.get("designFingerprints").is_none());
    assert!(
        captured_fingerprint(payload())
            .await
            .starts_with("v1:sha256:")
    );
}

#[tokio::test]
async fn fingerprint_manifest_field_in_extra_uses_converter_lookup() {
    let mut changed = payload();
    let node = changed.snapshot.nodes.get_mut("1:1").unwrap();
    let width = node.fields.remove("width").unwrap();
    node.extra.insert("width".into(), width);
    assert_eq!(
        captured_fingerprint(payload()).await,
        captured_fingerprint(changed).await
    );
}

#[tokio::test]
async fn fingerprint_read_error_is_recorded_even_with_a_value() {
    let mut changed = payload();
    changed
        .snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .field_errors
        .insert("width".into(), "partial read".into());
    assert_ne!(
        captured_fingerprint(payload()).await,
        captured_fingerprint(changed).await
    );
}

#[tokio::test]
async fn fingerprint_error_message_wording_is_ignored() {
    let mut first = payload();
    first
        .snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .field_errors
        .insert("opacity".into(), "failure at time A".into());
    let mut second = first.clone();
    second
        .snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .field_errors
        .insert("opacity".into(), "failure at time B".into());
    assert_eq!(
        captured_fingerprint(first).await,
        captured_fingerprint(second).await
    );
}

#[tokio::test]
async fn fingerprint_array_order_remains_significant() {
    let mut first = payload();
    first
        .snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .fields
        .insert("dashPattern".into(), json!([2, 4]));
    let mut second = first.clone();
    second
        .snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .fields
        .insert("dashPattern".into(), json!([4, 2]));
    assert_ne!(
        captured_fingerprint(first).await,
        captured_fingerprint(second).await
    );
}

#[tokio::test]
async fn fingerprint_section_source_maps_include_nodes() {
    let mut op = operation(&["tsx", "sourceMap"]);
    if let PendingOperation::Export { all_screens, .. } = &mut op {
        *all_screens = true;
    }
    let result = project(section(2), op).await.unwrap();
    for frame in result["frames"].as_array().unwrap() {
        let id = frame["nodeId"].as_str().unwrap();
        assert!(frame["sourceMap"]["designFingerprints"][id].is_string());
    }
}
