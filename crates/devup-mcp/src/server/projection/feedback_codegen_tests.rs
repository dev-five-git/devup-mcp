#[tokio::test]
async fn feedback36_confirmed_pair_merges_despite_different_names() {
    let directory = std::env::temp_dir().join(format!("feedback36-{}", rand::random::<u64>()));
    std::fs::create_dir_all(directory.join(".devup")).unwrap();
    std::fs::write(directory.join(".devup/screen-plan.json"), json!({
        "kind":"devup-screen-plan", "schemaVersion":1, "status":"confirmed",
        "routes":[{"route":"/technology", "nodeIds":["1:1", "1:2"]}]
    }).to_string()).unwrap();
    let mut data = responsive_payload();
    data.snapshot.nodes.get_mut("1:1").unwrap().fields.insert("name".into(), json!("Small technology"));
    data.snapshot.nodes.get_mut("1:2").unwrap().fields.insert("name".into(), json!("Technology overview"));
    let mut op = operation(&["responsiveTsx"]);
    if let PendingOperation::Export { frame_ids, theme_options, .. } = &mut op {
        *frame_ids = vec!["1:1".into(), "1:2".into()];
        theme_options.project_root = Some(directory.to_string_lossy().into_owned());
    }
    let result = project(data, op).await.unwrap();
    assert!(result["responsiveTsx"].is_string(), "{result}");
    assert_eq!(result["responsiveSlots"], json!([0,4]));
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn feedback36_refusal_preserves_frame_files_and_artifact_recovery() {
    let directory = std::env::temp_dir().join(format!("feedback36-files-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&directory).unwrap();
    let mut data = responsive_payload();
    data.snapshot.nodes.get_mut("1:2").unwrap().fields.insert("width".into(), json!(400));
    let mut op = operation(&["tsx", "responsiveTsx"]);
    if let PendingOperation::Export { frame_ids, output_paths, .. } = &mut op {
        *frame_ids = vec!["1:1".into(), "1:2".into()];
        output_paths.insert("frame:1:1:tsx".into(), directory.join("mobile.tsx").to_string_lossy().into_owned());
    }
    let result = project(data, op).await.unwrap();
    assert!(std::fs::read_to_string(directory.join("mobile.tsx")).unwrap().contains("export"));
    let failure = result["failures"].as_array().unwrap().iter().find(|failure| failure["output"] == "responsiveTsx").unwrap();
    assert_eq!(failure["details"]["frameReasons"].as_array().unwrap().len(), 2);
    assert_eq!(failure["details"]["nextAction"]["arguments"]["artifactId"], result["cache"]["artifactId"]);
    assert_eq!(failure["details"]["nextAction"]["arguments"]["outputs"], json!(["tsx"]));
    assert_ne!(result["status"], "complete");
    std::fs::remove_dir_all(directory).unwrap();
}
