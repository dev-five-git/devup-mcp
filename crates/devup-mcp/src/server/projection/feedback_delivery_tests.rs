#[tokio::test]
async fn feedback35_oversized_selection_commits_requested_file() {
    let directory = std::env::temp_dir().join(format!("devup-feedback35-{:016x}", rand::random::<u64>()));
    std::fs::create_dir(&directory).unwrap();
    let target = directory.join("payload.json");
    let mut data = payload();
    data.metadata = json!({"large": "x".repeat(100_000)});
    let mut request = operation(&["rawPayload"]);
    if let PendingOperation::Export { output_paths, response_selection, .. } = &mut request {
        output_paths.insert("rawPayload".into(), target.to_string_lossy().into_owned());
        response_selection.response_fields = vec!["rawPayload".into()];
    }
    let response = project(data, request).await.unwrap();
    assert_eq!(response["written"], true);
    assert_eq!(response["outputPaths"]["rawPayload"], json!(crate::test_paths::canonical(&target)));
    assert!(response["responseTruncated"].is_object());
    assert!(response["artifactId"].is_string());
    let saved: Value = serde_json::from_slice(&std::fs::read(&target).unwrap()).unwrap();
    assert!(serde_json::to_vec(&saved).unwrap().len() > 65_536);
    assert!(response["responseTruncated"]["nextAction"]["arguments"].get("responseFields").is_none());
    std::fs::remove_dir_all(directory).unwrap();
}
