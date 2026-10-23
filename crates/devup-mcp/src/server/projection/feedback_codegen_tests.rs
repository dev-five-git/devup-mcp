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

fn feedback32_component_payload() -> CollectedPayload {
    let mut data = payload();
    data.target.node_id = Some("set".into());
    data.snapshot = serde_json::from_value(json!({
        "fileKey":"W1Fixture","version":null,"roots":["set"],"diagnostics":[],
        "nodes":{
            "set":{"id":"set","type":"COMPONENT_SET","fields":{
                "name":"Button","width":900,"height":600,"childrenIds":["default","hover"],
                "componentPropertyDefinitions":{"effect":{"type":"VARIANT","defaultValue":"default","variantOptions":["default","hover"]}}
            }},
            "default":{"id":"default","type":"COMPONENT","fields":{
                "name":"effect=default","parentId":"set","childrenIds":["label"],
                "variantProperties":{"effect":"default"},"width":100,"height":40,
                "fills":[{"type":"SOLID","color":{"r":1,"g":0,"b":0}}]
            }},
            "hover":{"id":"hover","type":"COMPONENT","fields":{
                "name":"effect=hover","parentId":"set","childrenIds":["hover-label"],
                "variantProperties":{"effect":"hover"},"width":100,"height":40,
                "fills":[{"type":"SOLID","color":{"r":0,"g":0,"b":1}}]
            }},
            "label":{"id":"label","type":"TEXT","fields":{"characters":"Submit","parentId":"default"}},
            "hover-label":{"id":"hover-label","type":"TEXT","fields":{"characters":"Submit","parentId":"hover"}}
        }
    })).unwrap();
    data
}

#[tokio::test]
async fn feedback32_component_set_has_no_canvas_coverage_failure() {
    let result = project(feedback32_component_payload(), operation(&["tsx"])).await.unwrap();
    let code = result["tsx"].as_str().unwrap();
    assert_eq!(code.matches("export function Button").count(), 1);
    assert!(!code.contains("900px"), "{code}");
    assert!(!result["projectionIssues"].as_array().into_iter().flatten().any(|issue|
        issue["code"] == "DEVUP_CODEGEN_SECTION_COVERAGE" && issue["nodeId"] == "set"
    ), "{result}");
}

#[tokio::test]
async fn feedback32_screen_reference_offers_component_export() {
    let mut data = feedback32_component_payload();
    data.target.node_id = Some("screen".into());
    data.snapshot.roots = vec!["screen".into()];
    for (id, node) in [
        ("screen", json!({"id":"screen","type":"FRAME","fields":{"name":"Screen","childrenIds":["instance"]}})),
        ("instance", json!({"id":"instance","type":"INSTANCE","fields":{"name":"Button","mainComponentId":"default","variantProperties":{"effect":"default"}}})),
    ] {
        data.snapshot.nodes.insert(id.into(), serde_json::from_value(node).unwrap());
    }
    let result = project(data, operation(&["componentTsx"])).await.unwrap();
    assert!(result["componentTsx"].as_str().unwrap().contains("@/components/Button"), "{result}");
    let action = &result["componentDefinitions"][0]["nextAction"];
    assert_eq!(action["tool"], "devup_figma_export");
    assert_eq!(action["arguments"]["frameIds"], json!(["set"]));
    assert_eq!(action["arguments"]["outputs"], json!(["componentTsx"]));
    assert_eq!(action["arguments"]["artifactId"], result["cache"]["artifactId"]);
}
