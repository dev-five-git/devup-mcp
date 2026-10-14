fn feedback_theme_payload() -> CollectedPayload {
    let mut data = payload();
    let node = data.snapshot.nodes.get_mut("1:1").unwrap();
    node.node_type = "TEXT".into();
    node.fields.insert("characters".into(), json!("Example"));
    node.fields.insert("textStyleId".into(), json!("good"));
    node.fields.insert("fontSize".into(), json!(16));
    node.fields.insert("lineHeight".into(), json!({"unit":"PERCENT","value":140}));
    node.fields.insert("layoutSizingHorizontal".into(), json!("HUG"));
    node.fields.insert("layoutSizingVertical".into(), json!("HUG"));
    node.fields.insert("textTruncation".into(), json!("DISABLED"));
    let raw = json!({"collections":[],"variables":[],"styles":[
        {"id":"good","name":"Body","styleType":"TEXT","value":{"fontSize":16,"lineHeight":{"unit":"PERCENT","value":140}}},
        {"id":"bad","name":"Unreadable Heading","styleType":"TEXT","value":{"lineHeight":{"$error":"cannot read"}}},
        {"id":"unused","name":"Unused","styleType":"TEXT","value":{"fontSize":32}}
    ]});
    data.variables = Some(devup_mcp_figma::UpstreamResult { raw: raw.clone() });
    data.styles = Some(devup_mcp_figma::UpstreamResult { raw });
    data
}

#[tokio::test]
async fn feedback_theme_partial_export_keeps_code_and_valid_styles_with_named_issue() {
    let mut data = feedback_theme_payload();
    data.snapshot.nodes.get_mut("1:1").unwrap().fields.insert("textStyleId".into(), json!("bad"));
    let mut op = operation(&["tsx", "devupJson"]);
    if let PendingOperation::Export { include_diagnostics, .. } = &mut op { *include_diagnostics = false; }
    let result = project(data, op).await.unwrap();
    assert!(result["tsx"].is_string());
    let theme: Value = serde_json::from_str(result["devupJson"].as_str().unwrap()).unwrap();
    assert!(theme["theme"]["typography"].get("body").is_some());
    assert_eq!(result["status"], "partial");
    assert_eq!(result["quality"]["theme"], "unresolved");
    assert_eq!(result["skippedStyles"][0]["name"], "Unreadable Heading");
    assert_eq!(result["skippedStyles"][0]["nodeIds"], json!(["1:1"]));
    let issue = result["projectionIssues"].as_array().unwrap().iter().find(|issue| issue["code"] == "DEVUP_THEME_STYLE_SKIPPED").unwrap();
    assert_eq!(issue["resourceId"], "bad");
    assert_eq!(issue["nodeId"], "1:1");
    assert!(issue["message"].as_str().unwrap().contains("Unreadable Heading"));
    assert!(result["warnings"].as_array().unwrap().iter().any(|warning| warning["code"] == "DEVUP_THEME_STYLE_SKIPPED"));
}

#[tokio::test]
async fn feedback_theme_strict_refuses_skipped_style_with_names() {
    let mut op = operation(&["devupJson"]);
    if let PendingOperation::Export { strict, .. } = &mut op { *strict = true; }
    let error = project(feedback_theme_payload(), op).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupThemeConflict);
    assert!(error.message.contains("Unreadable Heading"));
}

#[tokio::test]
async fn feedback_theme_used_only_is_derived_from_emitted_typography() {
    let mut op = operation(&["tsx", "devupJson", "sourceMap"]);
    if let PendingOperation::Export { theme_options, .. } = &mut op { theme_options.mode = "used-only".into(); }
    let result = project(feedback_theme_payload(), op).await.unwrap();
    let theme: Value = serde_json::from_str(result["devupJson"].as_str().unwrap()).unwrap();
    assert_eq!(theme["theme"]["typography"].as_object().unwrap().keys().collect::<Vec<_>>(), vec!["body"]);
    assert!(result["tsx"].as_str().unwrap().contains("typography=\"body\""));
    assert_eq!(result["devupJsonMode"], "used-only");
    assert_eq!(result["themeCounts"]["styles"], 1);
}

#[tokio::test]
async fn feedback_theme_missing_only_preserves_project_and_reports_conflict() {
    let directory = std::env::temp_dir().join(format!("devup-theme-feedback-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("package.json"), "{}").unwrap();
    let original = r#"{"theme":{"typography":{"body":{"fontSize":"18px"}}}}"#;
    std::fs::write(directory.join("devup.json"), original).unwrap();
    let mut op = operation(&["tsx", "devupJson"]);
    if let PendingOperation::Export { theme_options, .. } = &mut op {
        theme_options.mode = "missing-only".into();
        theme_options.project_root = Some(directory.to_string_lossy().into());
    }
    let result = project(feedback_theme_payload(), op).await.unwrap();
    let theme: Value = serde_json::from_str(result["devupJson"].as_str().unwrap()).unwrap();
    assert!(theme["theme"]["typography"].as_object().unwrap().is_empty());
    assert_eq!(result["themeDiff"]["existingTypographyConflicts"][0]["token"], "body");
    assert_eq!(result["themeDiff"]["visualFidelity"], "unverified");
    assert_eq!(result["quality"]["theme"], "conflicted");
    assert_eq!(std::fs::read_to_string(directory.join("devup.json")).unwrap(), original);
    std::fs::remove_dir_all(directory).unwrap();
}

fn feedback_responsive_section() -> CollectedPayload {
    let mut data = section(3);
    for (index, (name, width)) in [("Desktop", 1440), ("992~1280", 1280), ("Mobile", 360)].iter().enumerate() {
        let id = format!("1:{}", index + 1);
        let candidate = &mut data.metadata["sectionIndex"]["candidates"][index];
        candidate["name"] = json!(name);
        candidate["bounds"]["width"] = json!(width);
        let node = data.snapshot.nodes.get_mut(&id).unwrap();
        node.fields.insert("name".into(), json!(name));
        node.fields.insert("width".into(), json!(width));
        node.fields.insert("parentId".into(), json!("0:1"));
        node.fields.insert("layoutMode".into(), json!("VERTICAL"));
    }
    data
}

#[tokio::test]
async fn feedback_responsive_selection_discovers_groups_without_claiming_implementation() {
    let result = project(feedback_responsive_section(), operation(&["tsx"])).await.unwrap();
    assert_eq!(result["status"], "selection_required");
    assert_eq!(result["responsiveGroups"][0]["frameIds"], json!(["1:3", "1:2", "1:1"]));
    assert_eq!(result["responsiveGroups"][0]["nextAction"]["arguments"]["outputs"], json!(["responsiveTsx"]));
    assert_eq!(result["responsiveGroups"][0]["structureVerified"], false);
    assert_eq!(result["breakpointCompletion"]["remainingBreakpoints"].as_array().unwrap().len(), 3);
    assert_eq!(result["breakpointCompletion"]["implementedBreakpoints"], json!([]));
}

#[tokio::test]
async fn feedback_responsive_desktop_export_reports_two_remaining_widths() {
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export { frame_ids, .. } = &mut op { *frame_ids = vec!["1:1".into()]; }
    let result = project(feedback_responsive_section(), op).await.unwrap();
    assert_eq!(result["breakpointCompletion"]["exportedBreakpoints"].as_array().unwrap().len(), 1);
    assert_eq!(result["breakpointCompletion"]["remainingBreakpoints"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn feedback_responsive_merge_reports_all_exported_widths_and_still_requires_verification() {
    let mut op = operation(&["responsiveTsx"]);
    if let PendingOperation::Export { frame_ids, .. } = &mut op { *frame_ids = vec!["1:1".into(), "1:2".into(), "1:3".into()]; }
    let result = project(feedback_responsive_section(), op).await.unwrap();
    assert!(result["responsiveTsx"].is_string());
    assert_eq!(result["breakpointCompletion"]["exportedBreakpoints"].as_array().unwrap().len(), 3);
    assert_eq!(result["breakpointCompletion"]["remainingBreakpoints"], json!([]));
    assert_eq!(result["breakpointCompletion"]["implementationState"], "unverified");
}

#[tokio::test]
async fn feedback_responsive_different_screen_names_are_not_grouped() {
    let result = project(section(3), operation(&["tsx"])).await.unwrap();
    assert_eq!(result["responsiveGroups"], json!([]), "duplicate width variants are ambiguous");
}

#[test]
fn feedback_theme_mode_validation_is_additive_and_rejects_unprovable_requests() {
    use theme_export::validate;
    assert!(validate("full", &["devupJson".into()], None).is_ok());
    assert!(validate("used-only", &["devupJson".into()], None).is_err());
    assert!(validate("missing-only", &["devupJson".into(), "tsx".into()], None).is_err());
    assert!(validate("unknown", &["devupJson".into()], None).is_err());
}

#[tokio::test]
async fn feedback_theme_resource_retry_preserves_mode_and_required_code_outputs() {
    let mut op = operation(&["tsx", "devupJson"]);
    if let PendingOperation::Export { theme_options, delivery, .. } = &mut op {
        theme_options.mode = "used-only".into();
        *delivery = DeliveryMode::Resource;
    }
    let result = project(feedback_theme_payload(), op).await.unwrap();
    let args = &result["nextAction"]["arguments"];
    assert_eq!(args["devupJsonMode"], "used-only");
    assert_eq!(args["outputs"], json!(["tsx", "devupJson"]));
    let outputs: Vec<String> = serde_json::from_value(args["outputs"].clone()).unwrap();
    assert!(theme_export::validate(args["devupJsonMode"].as_str().unwrap(), &outputs, None).is_ok());
}

#[tokio::test]
async fn feedback_theme_missing_only_refuses_missing_project_theme() {
    let directory = std::env::temp_dir().join(format!("devup-no-theme-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("package.json"), "{}").unwrap();
    let mut op = operation(&["tsx", "devupJson"]);
    if let PendingOperation::Export { theme_options, .. } = &mut op {
        theme_options.mode = "missing-only".into();
        theme_options.project_root = Some(directory.to_string_lossy().into());
    }
    let error = project(feedback_theme_payload(), op).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupInvalidInput);
    assert!(error.message.contains("existing devup.json"));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn feedback_theme_input_defaults_to_compatible_full_export() {
    let input: super::super::tools::FigmaExportInput = serde_json::from_value(json!({"url":"https://www.figma.com/design/test/Test?node-id=1-1"})).unwrap();
    assert_eq!(input.devup_json_mode, "full");
    assert_eq!(input.outputs, vec!["tsx"]);
}

#[tokio::test]
async fn feedback_theme_uncollected_used_reference_is_partial_and_never_invented() {
    let mut data = feedback_theme_payload();
    data.styles.as_mut().unwrap().raw["styles"][0]["value"]["boundVariables"] = json!({"fontSize":{"id":"missing-size"}});
    data.variables.as_mut().unwrap().raw["styles"][0]["value"]["boundVariables"] = json!({"fontSize":{"id":"missing-size"}});
    // A string token that the collected variables cannot represent: preserve
    // the reference and report its missing definition, rather than synthesize.
    data.variables.as_mut().unwrap().raw["variables"] = json!([{ "id":"missing-size","name":"UncollectedSize","resolvedType":"FLOAT","variableCollectionId":"missing","valuesByMode":{}}]);
    let mut op = operation(&["tsx", "devupJson"]);
    if let PendingOperation::Export { theme_options, .. } = &mut op { theme_options.mode = "used-only".into(); }
    let result = project(data, op).await.unwrap();
    assert_eq!(result["quality"]["theme"], "unresolved");
    assert_eq!(result["unresolvedUsedTokens"][0]["token"], "$uncollectedSize");
    assert_eq!(result["status"], "partial");
}
