#[test]
fn responsive_merge_diagnostics_survive_export_summary_without_opt_in() {
    let data = responsive_payload();
    let outputs = vec!["responsiveTsx".into()];
    let codes = [
        ("DEVUP_CODEGEN_RESPONSIVE_UNMATCHED", devup_mcp_figma::FidelityImpact::Approximated),
        ("DEVUP_CODEGEN_RESPONSIVE_DRAWN_WIDTH_EMPTY", devup_mcp_figma::FidelityImpact::Lossy),
    ];
    let diagnostics = codes.iter().map(|(code, impact)| Diagnostic {
        code: (*code).into(),
        message: "Responsive merge requires review".into(),
        node_id: Some("1:1".into()),
        property: Some("w".into()),
        details: Some(json!({"output":"responsiveTsx","slot":0,"width":390,
            "originalValue":"390px","nextAction":"Inspect responsive merge provenance."})),
        severity: Some(DiagnosticSeverity::Warning),
        fidelity_impact: Some(*impact),
        ..Diagnostic::default()
    }).collect();
    let (result,) = summarize_projection_diagnostics::summarize_projection_diagnostics(
        summarize_projection_diagnostics::ProjectionDiagnostics {
            payload: &data,
            outputs: &outputs,
            include_diagnostics: false,
            root_layout: devup_mcp_devup_ui::codegen::RootLayout::Standalone,
            asset_names_per_node: true,
            scope: &"node".into(),
            strict: false,
            frame_ids: &Vec::new(),
            all_screens: false,
            asset_captures: &Vec::new(),
            delivery: DeliveryMode::Inline,
            result: json!({"responsiveTsx":"export default function Page() { return null }"})
                .as_object().unwrap().clone(),
            failures: Vec::new(),
            warnings: Vec::new(),
            completeness_report: &data.completeness_report(),
            section_tsx_projected: false,
            projection_diagnostics: diagnostics,
            fidelity_reports: Vec::new(),
            theme_conflict_count: 0,
            theme_unresolved_count: 0,
            component_name_for_components: &None,
            target_kind: TargetKind::Screen,
        },
    ).unwrap();
    let mut result = Value::Object(result);
    super::super::review::attach(&mut result);
    for (code, impact) in codes {
        let issue = result["projectionIssues"].as_array().unwrap().iter()
            .find(|issue| issue["code"] == code).unwrap();
        assert_eq!(issue["details"]["output"], "responsiveTsx");
        assert_eq!(issue["fidelityImpact"], json!(impact));
        let review = result["reviewChecklist"].as_array().unwrap().iter()
            .find(|item| item["code"] == code).unwrap();
        assert_eq!(review["nodeIds"], json!(["1:1"]));
        assert_eq!(review["severity"], if impact == devup_mcp_figma::FidelityImpact::Lossy {
            "required"
        } else {
            "recommended"
        });
    }
}

#[tokio::test]
async fn instance_overrides_reach_export_and_required_review_without_opt_in() {
    for output in ["tsx", "componentTsx"] {
        let mut data = payload();
        let root = data.snapshot.roots[0].clone();
        let mut master = data.snapshot.nodes[&root].clone();
        master.id = "override-master".into();
        master.node_type = "COMPONENT".into();
        data.snapshot.nodes.insert(master.id.clone(), master);
        let instance = data.snapshot.nodes.get_mut(&root).unwrap();
        instance.node_type = "INSTANCE".into();
        instance.fields.insert("mainComponentId".into(), json!("override-master"));
        instance.fields.insert("overrides".into(), json!([{"id":root,"overriddenFields":["fills"]}]));
        instance.fields.insert("fills".into(), json!([{"type":"SOLID","color":{"r":0,"g":0.5,"b":0.5}}]));
        let mut result = project(data, operation(&[output])).await.unwrap();
        super::super::review::attach(&mut result);
        assert_eq!(result["instanceOverrides"][0]["instanceNodeId"], root, "{result}");
        assert_eq!(result["instanceOverrides"][0]["resolution"], "resolved");
        assert!(result[output].as_str().unwrap().contains("// @devup-override "));
        assert!(result["projectionIssues"].as_array().unwrap().iter().any(|item|
            item["code"] == "DEVUP_CODEGEN_INSTANCE_OVERRIDE"));
        assert!(result["reviewChecklist"].as_array().unwrap().iter().any(|item|
            item["code"] == "DEVUP_CODEGEN_INSTANCE_OVERRIDE" && item["severity"] == "required"));
    }
}

#[tokio::test]
async fn responsive_unmatched_diagnostic_reaches_export_review_without_opt_in() {
    let mut data = responsive_payload();
    for (root, children) in [
        ("1:1", vec!["small"]),
        ("1:2", vec!["wide-a", "wide-b"]),
    ] {
        data.snapshot.nodes.get_mut(root).unwrap().fields.insert("childrenIds".into(), json!(children));
        for child in children {
            data.snapshot.nodes.insert(child.into(), serde_json::from_value(json!({
                "id":child,"type":"FRAME","fields":{
                    "name":"Logo","parentId":root,"width":100,"height":24
                }
            })).unwrap());
        }
    }
    let mut op = operation(&["responsiveTsx"]);
    if let PendingOperation::Export { frame_ids, include_diagnostics, .. } = &mut op {
        *frame_ids = vec!["1:1".into(), "1:2".into()];
        *include_diagnostics = false;
    }
    let mut result = project(data, op).await.unwrap();
    super::super::review::attach(&mut result);
    assert!(result["responsiveTsx"].is_string(), "{result}");
    let code = "DEVUP_CODEGEN_RESPONSIVE_UNMATCHED";
    for items in [
        &result["projectionIssues"],
        &result["responsiveReviewChecklist"]["items"],
        &result["reviewChecklist"],
    ] {
        assert!(items.as_array().unwrap().iter().any(|item| item["code"] == code), "{items}");
    }
}

#[tokio::test]
async fn suspicious_design_values_are_published_for_recommended_review_without_loss() {
    let mut data = payload();
    let root = data.snapshot.roots[0].clone();
    let fields = &mut data.snapshot.nodes.get_mut(&root).unwrap().fields;
    fields.insert("width".into(), json!(171));
    fields.insert("height".into(), json!(80));
    fields.insert("layoutSizingHorizontal".into(), json!("FIXED"));
    fields.insert("layoutSizingVertical".into(), json!("FIXED"));
    fields.remove("parentType");
    fields.remove("childrenIds");
    let mut result = project(data, operation(&["tsx"])).await.unwrap();
    super::super::review::attach(&mut result);
    let issue = result["projectionIssues"].as_array().unwrap().iter()
        .find(|issue| issue["code"] == "DEVUP_CODEGEN_SUSPICIOUS_VALUE").expect("published advisory");
    assert_eq!(issue["fidelityImpact"], "none");
    assert_eq!(issue["details"]["appliedValue"], "171px");
    assert!(result["tsx"].as_str().unwrap().contains("w=\"171px\""));
    assert_ne!(result["quality"]["projection"], "lossy");
    assert!(result["reviewChecklist"].as_array().unwrap().iter().any(|item|
        item["code"] == "DEVUP_CODEGEN_SUSPICIOUS_VALUE" && item["severity"] == "recommended"));
}

#[tokio::test]
async fn stale_plugin_warning_is_visible_without_optional_diagnostics() {
    let mut data = payload();
    data.snapshot.diagnostics.push(serde_json::from_value(json!({
        "code":"DEVUP_BRIDGE_PLUGIN_STALE", "message":"Plugin scripts are stale.",
        "severity":"warning", "fidelityImpact":"none",
        "details":{"pluginStale":true,"expected":"current","actual":"old"}
    })).unwrap());
    let result = project(data, operation(&["tsx"])).await.unwrap();
    let issues = result["projectionIssues"].as_array().unwrap();
    let warning = issues.iter().find(|issue| issue["code"] == "DEVUP_BRIDGE_PLUGIN_STALE").unwrap();
    assert_eq!(warning["fidelityImpact"], "none");
    assert_eq!(warning["details"]["pluginStale"], true);
}

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
    assert!(!std::fs::read_to_string(result["outputPaths"]["responsiveTsx"].as_str().unwrap()).unwrap().is_empty());
    assert!(result.get("responsiveTsx").is_none());
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
async fn feedback66_component_roots_export_despite_uncaptured_nested_master() {
    for root in ["set", "default", "instance"] {
        let mut data = feedback32_component_payload();
        data.snapshot.nodes.insert("nested".into(), serde_json::from_value(json!({
            "id":"nested","type":"INSTANCE","fields":{"name":"Icon","mainComponentId":"outside"}
        })).unwrap());
        data.snapshot.nodes.get_mut("default").unwrap().fields.insert("childrenIds".into(), json!(["label","nested"]));
        data.snapshot.nodes.insert("instance".into(), serde_json::from_value(json!({
            "id":"instance","type":"INSTANCE","fields":{"name":"Button","mainComponentId":"default"}
        })).unwrap());
        data.target.node_id = Some(root.into());
        data.snapshot.roots = vec![root.into()];
        let directory = std::env::temp_dir().join(format!("feedback66-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("Button.tsx");
        let mut op = operation(&["componentTsx", "sourceMap"]);
        if let PendingOperation::Export { output_paths, .. } = &mut op {
            output_paths.insert("componentTsx".into(), path.to_string_lossy().into_owned());
        }
        let result = project(data, op).await.unwrap();
        let code = std::fs::read_to_string(&path).unwrap();
        assert!(code.contains("export function Button"), "{root}: {result}");
        assert!(result.get("componentTsx").is_none());
        assert!(result["failures"].as_array().unwrap().is_empty(), "{root}: {result}");
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[tokio::test]
async fn feedback32_component_set_has_no_canvas_coverage_failure() {
    let result = project(feedback32_component_payload(), operation(&["tsx"])).await.unwrap();
    let code = result["tsx"].as_str().unwrap();
    assert_eq!(code.matches("export function Button").count(), 1);
    assert!(!code.contains("900px"), "{code}");
    assert!(!code.contains("600px"), "{code}");
    assert!(code.contains("_hover="), "{code}");
    assert!(!code.contains("effect:"), "{code}");
    assert!(!code.contains("dashed"), "{code}");
    assert!(!result["projectionIssues"].as_array().into_iter().flatten().any(|issue|
        issue["code"] == "DEVUP_CODEGEN_SECTION_COVERAGE" && issue["nodeId"] == "set"
    ), "{result}");
}

#[tokio::test]
async fn feedback64_node_identity_requires_explicit_opt_in() {
    for enabled in [false, true] {
        let mut op = operation(&["tsx", "componentTsx", "sourceMap"]);
        if let PendingOperation::Export { theme_options, .. } = &mut op {
            theme_options.figma_node_ids = enabled;
        }
        let result = project(payload(), op).await.unwrap();
        for output in ["tsx", "componentTsx"] {
            let code = result[output].as_str().unwrap();
            assert_eq!(code.contains("data-figma-node-id"), enabled, "{output}: {code}");
        }
        assert!(result["sourceMap"].is_object(), "{result}");
        let mut op = operation(&["responsiveTsx"]);
        if let PendingOperation::Export { theme_options, frame_ids, .. } = &mut op {
            theme_options.figma_node_ids = enabled;
            *frame_ids = vec!["1:1".into(), "1:2".into()];
        }
        let result = project(responsive_payload(), op).await.unwrap();
        let code = result["responsiveTsx"].as_str().unwrap();
        assert_eq!(code.contains("data-figma-node-id"), enabled, "{code}");
    }
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

#[tokio::test]
async fn feedback65_component_reference_uses_master_set_and_variant_props() {
    let mut data = feedback32_component_payload();
    data.target.node_id = Some("screen".into());
    data.snapshot.roots = vec!["screen".into()];
    for (id, node) in [
        ("screen", json!({"id":"screen","type":"FRAME","fields":{"name":"Screen","childrenIds":["instance"]}})),
        ("instance", json!({"id":"instance","type":"INSTANCE","fields":{"name":"Unrelated layer name","mainComponentId":"default","variantProperties":{"size":"sm","effect":"default"}}})),
    ] {
        data.snapshot.nodes.insert(id.into(), serde_json::from_value(node).unwrap());
    }
    let result = project(data, operation(&["componentTsx"])).await.unwrap();
    let code = result["componentTsx"].as_str().unwrap();
    assert!(code.contains("import { Button } from \"@/components/Button\""), "{code}");
    assert!(code.contains("<Button size={\"sm\"} />"), "{code}");
    assert!(!code.contains("effect="), "{code}");
    assert_eq!(result["componentDefinitions"][0]["nodeId"], "set");
}
