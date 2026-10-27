
include!("design_drift_tests.rs");
include!("feedback_theme_tests.rs");
include!("feedback_codegen_tests.rs");
include!("feedback_delivery_tests.rs");
use super::super::artifacts::ArtifactRequestKey;
use super::*;
use devup_mcp_figma::{CollectionRequest, CollectionScope};

#[test]
fn r13_mapping_gap_is_not_exact_or_value_loss() {
    let d = Diagnostic {
        code: "DEVUP_CODEGEN_PROPERTY_UNMAPPED".into(),
        fidelity_impact: Some(devup_mcp_figma::FidelityImpact::None),
        ..Default::default()
    };
    let quality = serde_json::to_value(projection_quality(true, &[d])).unwrap();
    assert_eq!(quality, "mapping-incomplete");
}

#[test]
fn r17_source_layout_gap_cannot_be_overwritten_by_complete_attribute_audit() {
    let mut result = json!({"tsx":"<Box/>","quality":{"projection":"lossy"},"status":"partial",
            "outputResults":{"tsx":{"projection":"lossy","fidelity":{"layout":{"total":1,"covered":0},"uncoveredLayout":["child#layoutSizingHorizontal"]},"_propertyContract":[]}},"projectionIssues":[]});
    audit_delivered_properties(&mut result);
    assert_eq!(result["mappingComplete"], false);
    assert_eq!(result["outputResults"]["tsx"]["mappingComplete"], false);
    assert_eq!(result["quality"]["projection"], "lossy");
}

#[test]
fn r13_review_malformed_postprocess_cannot_pass_an_empty_ast() {
    let mut result = json!({"tsx":"export const X = <Box", "quality":{"projection":"exact"},"status":"complete",
            "outputResults":{"tsx":{"projection":"exact","_propertyContract":[]}},"projectionIssues":[]});
    audit_delivered_properties(&mut result);
    assert_eq!(result["mappingComplete"], false);
    assert_ne!(result["quality"]["projection"], "exact");
}

#[tokio::test]
async fn r13_review_responsive_resource_preserves_selection_in_next_action() {
    let mut op = operation(&["responsiveTsx"]);
    if let PendingOperation::Export {
        all_screens,
        delivery,
        ..
    } = &mut op
    {
        *all_screens = true;
        *delivery = DeliveryMode::Resource;
    }
    let result = project(responsive_payload(), op).await.unwrap();
    assert_eq!(
        result["nextAction"]["arguments"]["outputs"],
        json!(["responsiveTsx"])
    );
    assert_eq!(result["nextAction"]["arguments"]["allScreens"], true);
}

#[tokio::test]
async fn r13_review_reference_resource_has_binary_size_guidance() {
    let store = ArtifactStore::default();
    let data = payload();
    let key = ArtifactRequestKey::from_collection(&CollectionRequest::new(
        data.target.clone(),
        CollectionScope::Node,
    ));
    let artifact = store.insert(key, data).await.unwrap();
    let mut result = json!({"referencePng":{"byteLength":900000,"mimeType":"image/png"}});
    let outputs = vec![ProjectedOutput::binary(
        "reference.png",
        "image/png",
        vec![0; 900000],
    )];
    let args =
        json!({"artifactId":artifact.artifact_id,"outputs":["referencePng"],"delivery":"resource"});
    let attachment = apply_delivery(
        &mut result,
        DeliveryMode::Resource,
        &store,
        &artifact,
        outputs,
        Some(&args),
    )
    .await
    .unwrap();
    assert_eq!(
        result["nextAction"]["arguments"]["outputs"],
        json!(["referencePng"])
    );
    assert_eq!(result["nextAction"]["sizeEstimate"]["outputBytes"], 900000);
    assert!(
        result["nextAction"]["sizeEstimate"]["minimumWireBytes"]
            .as_u64()
            .unwrap()
            >= 2400000
    );
    assert_eq!(result["nextAction"]["arguments"]["delivery"], "resource");
    commit_delivery(attachment);
}

#[tokio::test]
async fn r13_resource_next_action_executes_same_named_frame_without_upstream() {
    use super::super::{DevupAuth, DevupServer, Services};
    use devup_mcp_figma::{AuthStatus, FigmaUpstream, ReadToolCall, UpstreamResult};
    use std::sync::Arc;
    struct Offline;
    #[async_trait::async_trait]
    impl DevupAuth for Offline {
        async fn status(&self) -> Result<AuthStatus, DevupError> {
            panic!("unexpected authentication")
        }
        async fn login(&self) -> Result<AuthStatus, DevupError> {
            panic!("unexpected login")
        }
        async fn logout(&self) -> Result<AuthStatus, DevupError> {
            panic!("unexpected logout")
        }
    }
    #[async_trait::async_trait]
    impl FigmaUpstream for Offline {
        async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
            panic!("unexpected upstream discovery")
        }
        async fn call_read_tool(&self, _: ReadToolCall) -> Result<UpstreamResult, DevupError> {
            panic!("unexpected upstream read")
        }
    }
    let store = ArtifactStore::default();
    let mut data = section(2);
    data.metadata["selectedRootIds"] = json!(["1:1", "1:2"]);
    let key = ArtifactRequestKey::from_collection(&CollectionRequest::new(
        data.target.clone(),
        CollectionScope::Node,
    ));
    let artifact = store.insert(key, data).await.unwrap();
    let policy = OutputPolicy::from_roots(vec![std::env::temp_dir()]).unwrap();
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export {
        frame_ids,
        component_name,
        ..
    } = &mut op
    {
        *frame_ids = vec!["1:1".into(), "1:2".into()];
        *component_name = Some("AuditScreen".into());
    }
    let baseline = complete_operation(
        op.clone(),
        &artifact.payload,
        "artifact",
        &artifact,
        &policy,
        &store,
    )
    .await
    .unwrap();
    if let PendingOperation::Export { delivery, .. } = &mut op {
        *delivery = DeliveryMode::Resource;
    }
    let resource = complete_operation(
        op,
        &artifact.payload,
        "artifact",
        &artifact,
        &policy,
        &store,
    )
    .await
    .unwrap();
    let mut server = DevupServer::new(Services::new(Arc::new(Offline), Arc::new(Offline)));
    server.artifacts = store;
    let result = server
        .devup_figma_export(rmcp::handler::server::wrapper::Parameters(
            serde_json::from_value(resource["nextAction"]["arguments"].clone()).unwrap(),
        ))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(result["cache"]["artifactId"], artifact.artifact_id);
    assert_eq!(result["frames"].as_array().unwrap().len(), 1);
    assert_eq!(result["frames"][0]["tsx"], baseline["frames"][0]["tsx"]);
}

#[test]
fn r13_delivery_gate_detects_future_postprocessor_even_without_public_map() {
    let mut data = payload();
    for field in ["layoutSizingHorizontal", "layoutSizingVertical"] {
        data.snapshot
            .nodes
            .get_mut("1:1")
            .unwrap()
            .fields
            .insert(field.into(), json!("FIXED"));
    }
    let output = generate_component(&data.snapshot, "1:1", &CodegenOptions::default()).unwrap();
    assert_eq!(output_result(&output)["projection"], "exact");
    let mut result = json!({"tsx":output.tsx,"quality":{"projection":"exact"},"status":"complete",
            "deliverable":{"isFinal":true},"outputResults":{"tsx":output_result(&output)},"projectionIssues":[]});
    result["tsx"] = json!(
        output
            .tsx
            .replacen("<Box", "<Box futurePostprocess=\"retained\"", 1)
    );
    audit_delivered_properties(&mut result);
    assert_eq!(result["quality"]["projection"], "mapping-incomplete");
    assert_eq!(
        result["outputResults"]["tsx"]["projection"],
        "mapping-incomplete"
    );
    assert_eq!(result["deliverable"]["isFinal"], false);
    assert_eq!(result["projectionReview"]["groups"][0]["priority"], 2);
    assert!(
        result["tsx"]
            .as_str()
            .unwrap()
            .contains("futurePostprocess=\"retained\"")
    );
    assert!(
        result["projectionIssues"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |d| d["code"] == "DEVUP_CODEGEN_PROPERTY_UNMAPPED" && d["fidelityImpact"] == "none"
            )
    );
    assert!(
        result["outputResults"]["tsx"]
            .get("_propertyContract")
            .is_none()
    );
}

#[tokio::test]
async fn r13_strict_codegen_mapping_gap_keeps_property_diagnostics() {
    let mut data = payload();
    let node = data.snapshot.nodes.get_mut("1:1").unwrap();
    node.node_type = "TEXT".into();
    node.fields.extend(serde_json::from_value::<std::collections::BTreeMap<String, Value>>(json!({
            "characters":"ab", "textTruncation":"DISABLED",
            "styledTextSegments":[{"characters":"a","fontSize":16,"fontName":{"family":"Arial"}},{"characters":"b","fontSize":16,"fontName":{"family":"serif"}}]
        })).unwrap());
    let output = generate_component(&data.snapshot, "1:1", &CodegenOptions::default()).unwrap();
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_PROPERTY_UNMAPPED")
    );
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export { strict, .. } = &mut op {
        *strict = true;
    }
    let error = project(data, op).await.unwrap_err();
    assert!(
        error.details["projectionIssues"]
            .as_array()
            .is_some_and(|issues| issues.iter().any(|d| d["code"]
                == "DEVUP_CODEGEN_PROPERTY_UNMAPPED"
                && d["nodeId"] == "1:1"
                && d["property"] == "fontFamily"
                && d["fidelityImpact"] == "none"))
    );
}

#[tokio::test]
async fn r13_agreeing_widths_verify_the_responsive_merge() {
    let mut op = operation(&["responsiveTsx"]);
    if let PendingOperation::Export { all_screens, .. } = &mut op {
        *all_screens = true;
    }
    let result = project(responsive_payload(), op).await.unwrap();
    let responsive = &result["outputResults"]["responsiveTsx"];
    assert_eq!(responsive["mappingComplete"], true, "{result}");
    assert_eq!(responsive["fidelity"]["mergedMappingVerified"], true);
    assert!(
        !result["projectionIssues"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|d| d["details"]["output"] == "responsiveTsx"
                && d["code"] == "DEVUP_CODEGEN_PROPERTY_UNMAPPED")
    );
}

#[tokio::test]
async fn r13_responsive_mapping_gap_is_explicit_and_strict_rejects() {
    let mut op = operation(&["responsiveTsx"]);
    if let PendingOperation::Export { all_screens, .. } = &mut op {
        *all_screens = true;
    }
    // Only the mobile width is filled. devup-ui keeps a value into the
    // wider slots, so the desktop width is drawn red where it is not.
    let mut data = responsive_payload();
    data.snapshot.nodes.get_mut("1:1").unwrap().fields.insert(
        "fills".into(),
        json!([{"type":"SOLID","color":{"r":1,"g":0,"b":0}}]),
    );
    let result = project(data.clone(), op.clone()).await.unwrap();
    assert!(result["responsiveTsx"].is_string());
    assert_eq!(result["mappingComplete"], false);
    let merge = result["projectionIssues"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|d| {
            d["details"]["unmappedProperties"]
                .as_array()
                .into_iter()
                .flatten()
        })
        .find(|p| p["reason"] == "unverified-responsive-merge")
        .unwrap();
    assert!(
        merge["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gap| gap["property"] == "bg" && gap["slot"] == 4 && gap["widthNodeId"] == "1:2"),
        "{merge}"
    );
    assert_ne!(
        result["outputResults"]["responsiveTsx"]["projection"],
        "exact"
    );
    assert!(
        result["projectionIssues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "DEVUP_CODEGEN_PROPERTY_UNMAPPED"
                && d["details"]["output"] == "responsiveTsx"
                && d["fidelityImpact"] == "none")
    );
    if let PendingOperation::Export { strict, .. } = &mut op {
        *strict = true;
    }
    assert!(project(data, op).await.is_err());
}

#[tokio::test]
async fn r13_final_frame_contract_covers_both_outputs_after_filename_rewrite() {
    let mut data = payload();
    data.snapshot = serde_json::from_str(include_str!(
        "../../../../../fixtures/r13/f13-1-snapshot.json"
    ))
    .unwrap();
    data.target.node_id = Some("3997:46715".into());
    let mut op = operation(&["tsx", "componentTsx", "sourceMap"]);
    if let PendingOperation::Export {
        include_diagnostics,
        ..
    } = &mut op
    {
        *include_diagnostics = false;
    }
    let result = project(data, op).await.unwrap();
    for field in ["tsx", "componentTsx"] {
        let code = result[field].as_str().unwrap();
        let entries = result["sourceMap"]["byOutput"][field].as_array().unwrap();
        for attr in devup_mcp_devup_ui::provenance::attributes::generated_attributes(code) {
            assert!(
                entries
                    .iter()
                    .any(|e| e["generatedProperty"] == attr.source)
                    || result["projectionEvidence"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|d| d["details"]["output"] == field
                            && d["details"]["generatedProperty"] == attr.source),
                "{field}: {}",
                attr.source
            );
        }
    }
}

#[tokio::test]
async fn r13_asset_derivations_survive_map_and_diagnostics_opt_out() {
    let mut data = payload();
    data.snapshot = serde_json::from_str(include_str!(
        "../../../../../fixtures/r13/f13-1-snapshot.json"
    ))
    .unwrap();
    data.target.node_id = Some("3997:46715".into());
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export {
        include_diagnostics,
        ..
    } = &mut op
    {
        *include_diagnostics = false;
    }
    let result = project(data, op).await.unwrap();
    assert!(result.get("sourceMap").is_none());
    for prop in ["boxShadow", "objectFit", "objectPos"] {
        let evidence = result["projectionEvidence"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["nodeId"] == "3997:46741" && d["property"] == prop)
            .unwrap_or_else(|| panic!("Missing asset evidence: {prop}"));
        assert_eq!(evidence["fidelityImpact"], "none");
        assert_eq!(evidence["details"]["output"], "tsx");
        assert!(
            evidence["details"]["sourceFields"]
                .as_array()
                .is_some_and(|fields| !fields.is_empty())
        );
        assert!(evidence["details"]["calculation"].is_string());
    }
}

#[tokio::test]
async fn r13_frame_output_paths_report_unsupported_and_write_supported_keys() {
    let dir = std::env::temp_dir().join(format!("r13-paths-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("frame.tsx");
    let expected = crate::test_paths::canonical(&dir).join("frame.tsx");
    let mut op = operation(&["tsx", "sourceMap"]);
    if let PendingOperation::Export {
        frame_ids,
        output_paths,
        ..
    } = &mut op
    {
        *frame_ids = vec!["1:1".into()];
        output_paths.insert(
            "tsx".into(),
            dir.join("ignored.tsx").to_string_lossy().into(),
        );
        output_paths.insert("frame:1:1:tsx".into(), target.to_string_lossy().into());
        output_paths.insert("typo".into(), dir.join("typo").to_string_lossy().into());
    }
    let result = project(section(2), op).await.unwrap();
    assert_eq!(
        result["outputPathResults"]["diagnostics"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        result["outputPathResults"]["supportedKeys"]
            .as_array()
            .unwrap()
            .contains(&json!("frame:1:1:tsx"))
    );
    assert!(!std::fs::read_to_string(&target).unwrap().is_empty());
    assert!(result["frames"][0].get("tsx").is_none());
    assert_eq!(result["frames"][0]["outputResults"]["tsx"]["bodyDelivery"], "written-to-file");
    assert_eq!(
        result["outputPaths"]["frame:1:1:tsx"],
        expected.to_string_lossy().as_ref()
    );
    assert!(!dir.join("ignored.tsx").exists());
    std::fs::remove_file(target).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[tokio::test]
async fn r13_explicit_resource_offers_single_screen_single_output_reprojection() {
    let mut op = operation(&["tsx", "rawSnapshot", "sourceMap"]);
    if let PendingOperation::Export {
        frame_ids,
        delivery,
        ..
    } = &mut op
    {
        *frame_ids = vec!["1:1".into(), "1:2".into()];
        *delivery = DeliveryMode::Resource;
    }
    let result = project(section(2), op).await.unwrap();
    let action = &result["nextAction"];
    assert_eq!(action["tool"], "devup_figma_export");
    assert!(action["arguments"]["artifactId"].is_string());
    assert_eq!(action["arguments"]["frameIds"], json!(["1:1"]));
    assert_eq!(action["arguments"]["outputs"], json!(["tsx"]));
    assert_eq!(action["arguments"]["delivery"], "inline");
    assert!(action["sizeEstimate"]["outputBytes"].as_u64().unwrap() > 0);
    assert_eq!(action["sizeEstimate"]["isMeasuredReprojection"], false);
}

#[tokio::test]
async fn r10_content_sizing_uncertainty_survives_diagnostics_opt_out() {
    let data: CollectedPayload = serde_json::from_str(include_str!(
        "../../../../../fixtures/r2/wquw-118-payload.json"
    ))
    .unwrap();
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export {
        frame_ids,
        include_diagnostics,
        ..
    } = &mut op
    {
        *frame_ids = vec!["3997:46129".into()];
        *include_diagnostics = false;
    }
    let result = project(data, op).await.unwrap();
    for owner in [&result, &result["frames"][0]] {
        let evidence: Vec<_> = owner["projectionEvidence"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["details"]["resolution"] == "accounted-for-content-sizing")
            .collect();
        assert_eq!(evidence.len(), 2);
        for d in evidence {
            assert_eq!(d["screenId"], "3997:46129");
            assert_eq!(
                d["details"]["verification"]["reasonCode"],
                "font-metrics-not-measured"
            );
            assert_eq!(d["fidelityImpact"], "none");
        }
    }
}

#[tokio::test]
async fn r10_diagnostics_identify_exportable_screen() {
    let data: CollectedPayload = serde_json::from_str(include_str!(
        "../../../../../fixtures/r2/wquw-118-payload.json"
    ))
    .unwrap();
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export {
        frame_ids,
        include_diagnostics,
        ..
    } = &mut op
    {
        *frame_ids = vec!["3997:46129".into()];
        *include_diagnostics = true;
    }
    let result = project(data, op).await.unwrap();
    for key in ["diagnostics", "projectionIssues", "placementContracts"] {
        for d in result["frames"][0][key].as_array().unwrap() {
            assert_eq!(d["screenId"], "3997:46129", "{d}");
        }
    }
    for d in result["projectionIssues"].as_array().unwrap() {
        assert_eq!(d["screenId"], "3997:46129", "{d}");
    }
}

#[tokio::test]
async fn r10_resource_note_does_not_claim_nonfinal_tsx_is_final() {
    let data: CollectedPayload = serde_json::from_str(include_str!(
        "../../../../../fixtures/r2/wquw-120-payload.json"
    ))
    .unwrap();
    let mut op = operation(&["tsx", "componentTsx"]);
    if let PendingOperation::Export {
        frame_ids,
        delivery,
        ..
    } = &mut op
    {
        *frame_ids = vec!["3997:46582".into()];
        *delivery = DeliveryMode::Resource;
    }
    let result = project(data, op).await.unwrap();
    assert_eq!(result["deliverable"]["isFinal"], false);
    assert!(
        !result["deliverable"]["note"]
            .as_str()
            .unwrap()
            .contains("final TSX")
    );
}

#[tokio::test]
async fn r10_descendant_selection_has_corrected_arguments() {
    let mut data = section(2);
    data.snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .fields
        .insert("childrenIds".into(), json!(["child"]));
    data.snapshot.nodes.insert(
        "child".into(),
        serde_json::from_value(
            json!({"id":"child","type":"TEXT","fields":{"parentId":"1:1","characters":"hello"}}),
        )
        .unwrap(),
    );
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export { frame_ids, .. } = &mut op {
        *frame_ids = vec!["child".into()];
    }
    let error = project(data.clone(), op).await.unwrap_err();
    assert_eq!(
        error.details["nextAction"]["arguments"]["frameIds"],
        json!(["1:1"]),
        "{error:?}"
    );
    assert_eq!(
        error.details["selectionIssues"][0]["reason"],
        "descendant-of-screen"
    );
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export { frame_ids, .. } = &mut op {
        *frame_ids = vec!["absent".into()];
    }
    let error = project(data, op).await.unwrap_err();
    assert_eq!(
        error.details["selectionIssues"][0]["reason"],
        "not-found-in-section"
    );
    assert!(error.details["nextAction"].is_null());
}

#[tokio::test]
async fn r3_responsive_has_its_own_fidelity_result() {
    let mut op = operation(&["responsiveTsx"]);
    if let PendingOperation::Export { all_screens, .. } = &mut op {
        *all_screens = true;
    }
    let result = project(responsive_payload(), op).await.unwrap();
    assert_eq!(
        result["outputResults"]["responsiveTsx"]["state"],
        "produced"
    );
    let fidelity = &result["outputResults"]["responsiveTsx"]["fidelity"];
    assert_eq!(fidelity["scope"], "breakpoint-source-projections");
    assert_eq!(fidelity["breakpoints"].as_array().unwrap().len(), 2);
    assert_eq!(fidelity["mergedMappingVerified"], true);
}

#[tokio::test]
async fn r2_responsive_hidden_instance_and_visible_desktop() {
    for instance in [true, false] {
        let mut data = responsive_payload();
        for i in 1..=2 {
            let root = format!("1:{i}");
            let child = format!("2:{i}");
            data.snapshot
                .nodes
                .get_mut(&root)
                .unwrap()
                .fields
                .insert("childrenIds".into(), json!([child]));
            data.snapshot.nodes.insert(
                child.clone(),
                serde_json::from_value(json!({
                    "id":child,"type":if instance {"INSTANCE"} else {"FRAME"},
                    "fields":{"parentId":root,"name":"Logo","width":24,"height":24,
                        "opacity":if instance || i == 1 {0} else {1},
                        "fills":[{"type":"IMAGE","imageHash":"logo","scaleMode":"TILE"}]}
                }))
                .unwrap(),
            );
        }
        let mut op = operation(&["responsiveTsx"]);
        if let PendingOperation::Export { all_screens, .. } = &mut op {
            *all_screens = true;
        }
        let result = project(data, op).await.unwrap();
        let code = result["responsiveTsx"]
            .as_str()
            .expect("responsive TSX exists");
        devup_mcp_devup_ui::validation::validate_tsx(code).unwrap();
        assert!(
            code.contains("visibility="),
            "hidden layout must survive: {code}"
        );
        if instance {
            assert!(!code.contains("/images/"), "{code}");
        } else {
            // Different paint prop sets can produce separate responsive
            // branches. The desktop branch must then be independently
            // visible instead of inheriting the hidden mobile box.
            let desktop = &code[code.find("bg=").expect("desktop paint")..];
            assert!(
                code.contains("\"initial\"")
                    || (!desktop.contains("visibility=\"hidden\"") && code.contains("\"block\"")),
                "desktop must restore paint: {code}"
            );
            assert!(code.contains("/images/"), "desktop paint retained: {code}");
        }
    }
}

#[tokio::test]
async fn r2_nonrendering_background_and_responsive_keep_code() {
    for scale in ["FILL", "TILE", "BACKGROUND"] {
        for field in ["tsx", "componentTsx", "responsiveTsx"] {
            let mut data = responsive_payload();
            for i in 1..=2 {
                let root = format!("1:{i}");
                let child = format!("2:{i}");
                data.snapshot
                    .nodes
                    .get_mut(&root)
                    .unwrap()
                    .fields
                    .insert("childrenIds".into(), json!([child]));
                data.snapshot.nodes.insert(child.clone(), serde_json::from_value(json!({"id":child,"type":if field == "componentTsx" { "INSTANCE" } else { "FRAME" },
                        "fields":{"parentId":root,"name":"Transparent paint","width":24,"height":24,"visible":true,"opacity":0,
                            "fills":[{"type":"IMAGE","imageHash":"hidden","scaleMode":if scale == "BACKGROUND" { "FILL" } else { scale }}]}})).unwrap());
                if scale == "BACKGROUND" {
                    let text_id = format!("3:{i}");
                    data.snapshot
                        .nodes
                        .get_mut(&child)
                        .unwrap()
                        .fields
                        .insert("childrenIds".into(), json!([text_id]));
                    data.snapshot.nodes.insert(text_id.clone(), serde_json::from_value(json!({"id":text_id,"type":"TEXT",
                            "fields":{"parentId":child,"characters":"Retained text","width":20,"height":12}})).unwrap());
                }
            }
            let mut op = operation(&[field, "assetManifest"]);
            if let PendingOperation::Export { all_screens, .. } = &mut op {
                *all_screens = true;
            }
            let result = project(data, op).await.unwrap();
            assert!(
                result["failures"].as_array().unwrap().is_empty(),
                "{scale}/{field}: {result}"
            );
            if field != "responsiveTsx" {
                for frame in result["frames"].as_array().unwrap() {
                    let code = frame[field]
                        .as_str()
                        .expect("background exclusion must retain code");
                    assert!(!code.contains("/images/"));
                    assert!(code.contains("visibility=\"hidden\""), "{field}: {code}");
                    devup_mcp_devup_ui::validation::validate_tsx(code).unwrap();
                }
            } else {
                let code = result[field]
                    .as_str()
                    .expect("responsive exclusion must retain code");
                assert!(!code.contains("/images/"));
                devup_mcp_devup_ui::validation::validate_tsx(code).unwrap();
            }
        }
    }
}

#[tokio::test]
async fn r2_production_six_frames_return_valid_story_tsx() {
    let data: CollectedPayload = serde_json::from_str(include_str!(
        "../../../../../fixtures/r2/wquw-119-payload.json"
    ))
    .unwrap();
    let generated = generate_collected_component(
        &data,
        "3997:46690",
        &CodegenOptions {
            inline_instances: true,
            asset_names_per_node: true,
            ..CodegenOptions::default()
        }
        .with_payload_tokens(&data),
    )
    .unwrap();
    let path = devup_mcp_devup_ui::codegen::image_fill_path(&data.snapshot, "3997:46703", 0, true)
        .unwrap();
    eprintln!(
        "story pre-guard excluded path {path}; referenced={}",
        generated.tsx.contains(&path)
    );
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export {
        frame_ids,
        include_diagnostics,
        ..
    } = &mut op
    {
        *frame_ids = [
            "3997:46315",
            "3997:46715",
            "3997:46333",
            "3997:46361",
            "3997:46461",
            "3997:46690",
        ]
        .map(str::to_owned)
        .to_vec();
        *include_diagnostics = false;
    }
    let result = project(data, op).await.unwrap();
    assert_eq!(result["frames"].as_array().unwrap().len(), 6);
    for frame in result["frames"].as_array().unwrap() {
        let tsx = frame["tsx"]
            .as_str()
            .expect("every selected frame must retain TSX");
        devup_mcp_devup_ui::validation::validate_tsx(tsx).unwrap();
        assert!(!tsx.contains(&host_safe_asset_path(&path)));
        if frame["nodeId"] == "3997:46690" {
            assert!(tsx.contains("작은 시장, 큰 사랑"));
            assert!(tsx.contains("visibility=\"hidden\""));
            assert!(tsx.contains("boxSize=\"24px\""));
        }
    }
    assert!(result["failures"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn r2_withheld_output_has_issue_and_explicit_deliverable() {
    let mut data = payload();
    data.snapshot.nodes.get_mut("1:1").unwrap().node_type = "VECTOR".into();
    data.assets.push(serde_json::from_value(json!({"assetId":"1:1:node", "nodeId":"1:1",
            "field":"node", "sourceKind":"node", "status":"failed", "errorCode":"DEVUP_ASSET_NODE_HIDDEN"})).unwrap());
    let result = project(data, operation(&["tsx", "assetManifest"]))
        .await
        .unwrap();
    assert_eq!(result["deliverable"]["isFinal"], false);
    assert_eq!(result["deliverable"]["withheldOutputs"][0]["output"], "tsx");
    let issue = result["projectionIssues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["code"] == "DEVUP_EXCLUDED_ASSET_REFERENCED")
        .unwrap();
    assert_eq!(issue["code"], "DEVUP_EXCLUDED_ASSET_REFERENCED");
    assert_eq!(issue["property"], "assetReference");
    assert!(issue["details"]["originalValue"].is_object());
    assert_eq!(issue["details"]["appliedValue"]["state"], "withheld");
    assert!(issue["details"]["nextAction"].is_string());
}

#[tokio::test]
async fn r2_component_layout_has_generated_evidence_and_output_scope() {
    let mut data = section(1);
    data.snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .fields
        .insert("childrenIds".into(), json!(["2:1"]));
    data.snapshot.nodes.insert(
        "2:1".into(),
        serde_json::from_value(json!({
            "id":"2:1","type":"INSTANCE","fields":{"name":"Header", "parentId":"1:1",
            "visible":true,"width":360,"height":66,"childrenIds":[]}
        }))
        .unwrap(),
    );
    let mut op = operation(&["tsx", "componentTsx"]);
    if let PendingOperation::Export {
        all_screens,
        include_diagnostics,
        ..
    } = &mut op
    {
        *all_screens = true;
        *include_diagnostics = false;
    }
    let result = project(data, op).await.unwrap();
    let frame = &result["frames"][0];
    let issues = frame["projectionIssues"].as_array().unwrap();
    for issue in issues
        .iter()
        .filter(|i| i["code"] == "DEVUP_CODEGEN_LAYOUT_UNCOVERED")
    {
        assert!(issue["details"]["appliedValue"].is_object(), "{issue}");
        assert!(issue["details"]["output"].is_string(), "{issue}");
        assert!(issue["details"]["nextAction"].is_string(), "{issue}");
    }
    assert!(frame["outputResults"]["tsx"]["fidelity"].is_object());
    assert!(frame["outputResults"]["componentTsx"]["fidelity"].is_object());
    assert!(
        !frame["projectionReview"]["groups"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn r5_wquw119_missing_mask_size_is_output_scoped_and_not_final() {
    let mut data = payload();
    data.snapshot = serde_json::from_str(include_str!(
        "../../../../../fixtures/wquw-119-snapshot.json"
    ))
    .unwrap();
    data.target.node_id = Some("3997:46315".into());
    data.target.file_key = data.snapshot.file_key.clone();
    let node = data.snapshot.nodes.get_mut("3997:46317").unwrap();
    node.fields.remove("height");
    node.fields.remove("absoluteBoundingBox");
    let mut op = operation(&["tsx", "sourceMap"]);
    if let PendingOperation::Export {
        include_diagnostics,
        ..
    } = &mut op
    {
        *include_diagnostics = false;
    }
    let result = project(data, op).await.unwrap();
    assert_ne!(result["quality"]["projection"], "exact");
    assert_eq!(result["deliverable"]["isFinal"], false);
    assert!(
        result["projectionIssues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["nodeId"] == "3997:46317"
                && issue["property"] == "height"
                && issue["details"]["output"] == "tsx"),
        "{result}"
    );
}

#[tokio::test]
async fn r4_direct_export_discloses_capture_boundary_without_diagnostics() {
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export {
        include_diagnostics,
        ..
    } = &mut op
    {
        *include_diagnostics = false;
    }
    let result = project(payload(), op).await.unwrap();
    let contract = &result["placementContracts"][0]["details"];
    assert_eq!(contract["output"], "tsx");
    assert_eq!(contract["parentCollected"], false);
    assert!(contract["parentMissingReason"].is_string());
    assert_eq!(contract["containingBlock"], "normal-flow-host");
    assert!(contract["coordinateBasis"].is_string());
}

/// A screen exported on its own said nothing about the surface it is
/// drawn on, so learning that the desktop frame behind an app screen was
/// `#E6EDE6` took a second export of that parent — a second Figma
/// acquisition to read a colour this capture was already carrying.
#[tokio::test]
async fn r4_placement_contract_reports_the_collected_parent_background() {
    let mut data = payload();
    data.snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .fields
        .insert("parentId".into(), json!("0:1"));
    data.snapshot.nodes.insert(
        "0:1".into(),
        serde_json::from_value(json!({
            "id":"0:1", "type":"FRAME", "fields":{
                "name":"Desktop", "parentId":null, "childrenIds":["1:1"], "visible":true,
                "x":0, "y":0, "width":1920, "height":1080, "layoutMode":"NONE",
                "fills":[{"type":"SOLID", "visible":true, "opacity":1, "blendMode":"NORMAL",
                    "color":{"r":230.0/255.0, "g":237.0/255.0, "b":230.0/255.0}}]}
        }))
        .unwrap(),
    );
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export {
        include_diagnostics,
        ..
    } = &mut op
    {
        *include_diagnostics = false;
    }
    let result = project(data, op).await.unwrap();
    let details = &result["placementContracts"][0]["details"];
    assert_eq!(details["parentCollected"], true, "{result}");
    let background = &details["sourceParentBackground"];
    assert_eq!(background["color"], "#E6EDE6", "{result}");
    assert_eq!(background["name"], "Desktop", "{result}");
    assert!(background["note"].is_string(), "{result}");
    // Reported, never emitted: the parent is still not this component.
    assert_eq!(details["parentIncludedInOutput"], false, "{result}");
}
