
#[tokio::test]
async fn r4_export_exposes_placement_contract_without_diagnostics() {
    let data: CollectedPayload = serde_json::from_str(include_str!(
        "../../../../../fixtures/r2/wquw-120-payload.json"
    ))
    .unwrap();
    let mut op = operation(&["tsx", "componentTsx"]);
    if let PendingOperation::Export {
        frame_ids,
        include_diagnostics,
        ..
    } = &mut op
    {
        *frame_ids = vec!["3997:46582".into()];
        *include_diagnostics = false;
    }
    let result = project(data, op).await.unwrap();
    let frame = &result["frames"][0];
    for field in ["tsx", "componentTsx"] {
        let source = frame[field].as_str().unwrap();
        let tag = source
            .split("return (")
            .nth(1)
            .unwrap()
            .split('>')
            .next()
            .unwrap();
        for prop in ["pos=\"relative\"", "w=\"360px\"", "h=\"740px\""] {
            assert!(tag.contains(prop), "{tag}");
        }
        assert!(source.contains("whiteSpace=\"pre-wrap\""));
        let contract = frame["placementContracts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["details"]["output"] == field)
            .unwrap();
        assert_eq!(contract["details"]["containingBlock"], "generated-root");
    }
    assert_eq!(result["placementContracts"].as_array().unwrap().len(), 2);
    assert_eq!(
        frame["outputResults"]["tsx"]["fidelity"]["layout"]["covered"],
        134 // R17: prior 117 + 17 verified horizontal FILL obligations.
    );
    assert_eq!(
        frame["outputResults"]["componentTsx"]["fidelity"]["impacts"]["lossy"],
        40 // The paragraph now preserves its spaces; all other losses remain.
    );
    assert!(
        !frame["projectionIssues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| {
                issue["code"] == "DEVUP_CODEGEN_TEXT_WHITESPACE_COLLAPSE"
                    && issue["nodeId"] == "3997:46604"
                    && issue["property"] == "characters"
                    && issue["fidelityImpact"] == "lossy"
            })
    );
}

#[tokio::test]
async fn r9_confirmed_evidence_survives_diagnostics_opt_out() {
    let mut data: CollectedPayload = serde_json::from_str(include_str!(
        "../../../../../fixtures/r2/wquw-120-payload.json"
    ))
    .unwrap();
    data.snapshot
        .nodes
        .get_mut("3997:46621")
        .unwrap()
        .fields
        .insert("visible".into(), json!(false));
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export {
        frame_ids,
        include_diagnostics,
        ..
    } = &mut op
    {
        *frame_ids = vec!["3997:46582".into()];
        *include_diagnostics = false;
    }
    let result = project(data, op).await.unwrap();
    for owner in [&result, &result["frames"][0]] {
        let evidence = owner["projectionEvidence"]
            .as_array()
            .expect("confirmed evidence remains available without diagnostics");
        let d = evidence
            .iter()
            .find(|d| {
                d["nodeId"] == "3997:46621" && d["code"] == "DEVUP_CODEGEN_NON_RENDERING_ASSET"
            })
            .unwrap();
        assert_eq!(d["fidelityImpact"], "none");
        assert_eq!(d["details"]["verification"]["field"], "visible");
        assert_eq!(d["details"]["verification"]["state"], "accounted-for");
        assert_eq!(d["details"]["output"], "tsx");
        assert!(
            !owner["projectionIssues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["nodeId"] == "3997:46621")
        );
    }
}

#[tokio::test]
async fn r2_production_layout_evidence_is_actionable_per_output() {
    for (raw, ids, fields, count) in [
        (
            include_str!("../../../../../fixtures/r2/wquw-118-payload.json"),
            vec!["3997:46242", "3997:46277", "3997:46129"],
            vec!["tsx", "assetManifest"],
            0, // R10: both auto-size dimensions are accounted for, not lost.
        ),
        (
            include_str!("../../../../../fixtures/r2/wquw-120-payload.json"),
            vec!["3997:46582"],
            vec!["tsx", "componentTsx", "assetManifest"],
            40,
        ),
    ] {
        let data: CollectedPayload = serde_json::from_str(raw).unwrap();
        let mut op = operation(&fields);
        if let PendingOperation::Export {
            frame_ids,
            include_diagnostics,
            delivery,
            ..
        } = &mut op
        {
            *frame_ids = ids.into_iter().map(str::to_owned).collect();
            *include_diagnostics = false;
            *delivery = DeliveryMode::Auto;
        }
        let store = ArtifactStore::default();
        let artifact = store
            .insert(
                ArtifactRequestKey::from_collection(&CollectionRequest::new(
                    data.target.clone(),
                    CollectionScope::Node,
                )),
                data,
            )
            .await
            .unwrap();
        let mut result = complete_operation(
            op,
            &artifact.payload,
            "fixture",
            &artifact,
            &OutputPolicy::from_roots(vec![std::env::temp_dir()]).unwrap(),
            &store,
        )
        .await
        .unwrap();
        // Rich image evidence can promote this response to resources. Read
        // the retained complete diagnostic payload before asserting geometry.
        if let Some(resource) = store
            .output_manifests()
            .await
            .into_iter()
            .find(|m| m.name == "projection-details.json")
        {
            let mut bytes = Vec::new();
            for index in 0..resource.chunk_count {
                bytes.extend(
                    store
                        .read_resource_output(
                            &artifact.artifact_id,
                            &resource.output_id,
                            Some(index),
                        )
                        .await
                        .unwrap()
                        .1,
                );
            }
            let details: Value = serde_json::from_slice(&bytes).unwrap();
            for (field, value) in details["response"].as_object().unwrap() {
                result[field] = value.clone();
            }
            for frame in result["frames"].as_array_mut().into_iter().flatten() {
                if let Some(full) = details["frames"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|full| full["nodeId"] == frame["nodeId"])
                {
                    for (field, value) in full.as_object().unwrap() {
                        frame[field] = value.clone();
                    }
                }
            }
        }
        let issues = result["projectionIssues"].as_array().unwrap();
        for issue in issues
            .iter()
            .chain(result["projectionEvidence"].as_array().unwrap())
            .filter(|i| {
                matches!(
                    i["code"].as_str(),
                    Some("DEVUP_CODEGEN_ABSOLUTE_FALLBACK" | "DEVUP_CODEGEN_ABSOLUTE_VERIFIED")
                )
            })
        {
            let original = &issue["details"]["originalValue"];
            assert_eq!(original["x"], 0);
            assert_eq!(original["y"], 0);
            assert_eq!(original["width"], 360);
            assert_eq!(original["height"], 740);
            assert_eq!(original["parent"]["width"], 360);
            assert_eq!(original["constraints"]["horizontal"], "CENTER");
            assert_eq!(original["children"][0]["x"], 20);
            let padding = &issue["details"]["appliedValue"]["derivedPadding"];
            assert_eq!(padding["left"], 20.0);
            assert_eq!(padding["top"], if count == 40 { 232.5 } else { 185.5 });
        }
        let layout: Vec<_> = issues
            .iter()
            .filter(|i| i["code"] == "DEVUP_CODEGEN_LAYOUT_UNCOVERED")
            .collect();
        let original_class = if count == 40 {
            "component-reference"
        } else {
            "text-auto-size"
        };
        if count == 0 {
            assert_eq!(
                result["projectionEvidence"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|d| d["details"]["resolution"] == "accounted-for-content-sizing")
                    .count(),
                2
            );
        }
        assert_eq!(
            layout
                .iter()
                .filter(|i| i["details"]["classification"] == original_class)
                .count(),
            if count == 40 { 39 } else { count } // R17 fluid width now proves one previously missing mapping.
        );
        for issue in layout {
            assert_eq!(
                issue["details"]["appliedValue"]["state"], "emitted",
                "{issue}"
            );
            assert!(
                issue["details"]["appliedValue"]["generatedSource"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty())
            );
            assert!(issue["details"]["nextAction"].is_string());
            if issue["details"]["classification"] == "component-reference" {
                assert_eq!(issue["details"]["output"], "componentTsx");
                assert_eq!(issue["details"]["classification"], "component-reference");
            } else if issue["property"] == "layoutSizingHorizontal" {
                // R17 checks source FILL even when no width property was emitted.
                assert_eq!(issue["details"]["originalValue"], "FILL");
                assert_eq!(issue["fidelityImpact"], "lossy");
                assert_eq!(
                    issue["details"]["implicitCssVerification"]["state"],
                    "not-accounted-for"
                );
                assert!(issue["details"]["implicitCssVerification"]["reasonCode"].is_string());
            } else if issue["property"] == "layoutSizingVertical" {
                // R6 exposes a real percentage-height dependency that R2 did not count.
                assert_eq!(issue["nodeId"], "3997:46313");
                assert_eq!(issue["details"]["output"], "tsx");
                assert_eq!(issue["fidelityImpact"], "lossy");
                assert_eq!(issue["details"]["originalValue"], "FILL");
                assert_eq!(
                    issue["details"]["implicitCssVerification"]["state"],
                    "not-accounted-for"
                );
            } else if matches!(
                issue["details"]["classification"].as_str(),
                Some("asset-projection" | "property-unmapped")
            ) {
                // R5 additionally discloses unproven asset percentage axes.
                assert_eq!(issue["details"]["output"], "tsx");
                assert!(
                    matches!(issue["property"].as_str(), Some("width" | "height")),
                    "{issue}"
                );
            } else {
                assert_eq!(issue["details"]["output"], "tsx");
                assert_eq!(issue["details"]["classification"], "text-auto-size");
                assert!(
                    issue["details"]["appliedValue"]["generatedSource"]
                        .as_str()
                        .unwrap()
                        .contains("<Text")
                );
            }
        }
        if count == 40 {
            let frame = &result["frames"][0];
            // R14 proves the modal's width in its definite containing
            // block. Component-reference loss in componentTsx is separate.
            assert_eq!(frame["outputResults"]["tsx"]["projection"], "lossy");
            assert_eq!(frame["outputResults"]["tsx"]["mappingComplete"], false);
            assert!(
                frame["projectionIssues"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|i| i["property"] == "layoutSizingHorizontal"
                        && i["fidelityImpact"] == "lossy")
            );
            let modal = frame["projectionEvidence"]
                .as_array()
                .unwrap()
                .iter()
                .find(|d| {
                    d["nodeId"] == "3997:46621"
                        && d["code"] == "DEVUP_CODEGEN_ABSOLUTE_VERIFIED"
                        && d["details"]["output"] == "tsx"
                })
                .unwrap();
            assert_eq!(
                modal["details"]["components"]["width"]["generatedValue"],
                "100%"
            );
            assert_eq!(modal["details"]["components"]["width"]["state"], "verified");
            assert_eq!(
                frame["outputResults"]["componentTsx"]["projection"],
                "lossy"
            );
            let groups = frame["projectionReview"]["groups"].as_array().unwrap();
            assert_eq!(
                groups
                    .iter()
                    .filter(|g| g["classification"] == "component-reference")
                    .count(),
                5
            );
        }
    }
}

#[tokio::test]
async fn r1_hidden_asset_paths_absent_from_all_six_frames() {
    let mut data = section(6);
    data.snapshot
        .nodes
        .get_mut("1:6")
        .unwrap()
        .fields
        .insert("childrenIds".into(), json!(["3997:46703"]));
    data.snapshot.nodes.insert("3997:46703".into(), serde_json::from_value(json!({
            "id":"3997:46703","type":"FRAME","fields":{"name":"Hidden image", "parentId":"1:6",
            "visible":false,"width":24,"height":24,"fills":[{"type":"IMAGE","imageHash":"hidden","scaleMode":"FILL"}]}
        })).unwrap());
    let path = host_safe_asset_path(
        &devup_mcp_devup_ui::codegen::image_fill_path(&data.snapshot, "3997:46703", 0, true)
            .unwrap(),
    );
    let mut op = operation(&["tsx", "assetManifest"]);
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
    assert_eq!(result["frames"].as_array().unwrap().len(), 6);
    assert!(
        result["assetManifest"]["assets"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["assetId"] != "3997:46703:fills:0")
    );
    for frame in result["frames"].as_array().unwrap() {
        assert!(!frame["tsx"].as_str().unwrap().contains(&path), "{frame}");
    }
    assert_eq!(result["quality"]["assets"], "not-requested");
    assert!(result["assetSummary"]["description"].is_string());
}

#[tokio::test]
async fn r1_approximation_is_explained_without_diagnostics() {
    let mut data = section(1);
    let node = data.snapshot.nodes.get_mut("1:1").unwrap();
    node.fields
        .insert("layoutPositioning".into(), json!("ABSOLUTE"));
    node.fields
        .insert("relativeTransform".into(), json!([[1, 0, 0], [0.5, 1, 0]]));
    let mut op = operation(&["tsx"]);
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
    assert_eq!(frame["quality"]["projection"], "approximated");
    let issue = &frame["projectionIssues"][0];
    assert_eq!(issue["nodeId"], "1:1");
    assert_eq!(issue["property"], "layoutPositioning");
    assert_eq!(
        issue["details"]["originalValue"]["layoutPositioning"],
        "ABSOLUTE"
    );
    assert_eq!(
        issue["details"]["originalValue"]["relativeTransform"],
        json!([[1, 0, 0], [0.5, 1, 0]])
    );
    assert!(issue["details"]["originalValue"]["parentMissingReason"].is_string());
    assert!(issue["details"]["appliedValue"].is_object());
    assert!(issue["message"].is_string());
}

#[tokio::test]
async fn r1_uncovered_layout_cannot_be_exact() {
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
            "id":"2:1","type":"INSTANCE","fields":{"name":"Widget", "parentId":"1:1",
            "visible":true,"width":24,"height":24,"childrenIds":[]}
        }))
        .unwrap(),
    );
    let mut op = operation(&["tsx"]);
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
    if frame["fidelity"]["layout"]["covered"] != frame["fidelity"]["layout"]["total"] {
        assert_ne!(frame["quality"]["projection"], "exact");
        assert!(
            frame["projectionIssues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["code"] == "DEVUP_CODEGEN_LAYOUT_UNCOVERED")
        );
        assert_ne!(result["deliverable"]["isFinal"], true);
    } else {
        panic!("fixture must reproduce a layout shortfall: {frame}");
    }
}

#[tokio::test]
async fn r1_required_excluded_asset_withholds_code_and_finality() {
    let mut data = payload();
    data.snapshot.nodes.get_mut("1:1").unwrap().node_type = "VECTOR".into();
    data.assets.push(serde_json::from_value(json!({"assetId":"1:1:node", "nodeId":"1:1",
            "field":"node", "sourceKind":"node", "status":"failed", "errorCode":"DEVUP_ASSET_NODE_HIDDEN"})).unwrap());
    let result = project(data, operation(&["tsx", "assetManifest"]))
        .await
        .unwrap();
    assert!(result.get("tsx").is_none());
    assert_eq!(result["status"], "partial");
    assert_ne!(result["deliverable"]["isFinal"], true);
    assert!(
        result["failures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["errorCode"] == "DEVUP_EXCLUDED_ASSET_REFERENCED")
    );
}

#[tokio::test]
async fn r1_responsive_only_reports_source_approximation() {
    let mut data = responsive_payload();
    data.snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .fields
        .insert("layoutPositioning".into(), json!("ABSOLUTE"));
    let mut op = operation(&["responsiveTsx"]);
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
    assert_ne!(result["quality"]["projection"], "exact");
    assert!(
        result["projectionIssues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["nodeId"] == "1:1" && i["property"] == "layoutPositioning")
    );
}

#[tokio::test]
async fn r1_captured_asset_overrides_hidden_discovery_prediction() {
    let mut data = payload();
    let node = data.snapshot.nodes.get_mut("1:1").unwrap();
    node.node_type = "VECTOR".into();
    node.fields.insert(
        "absoluteBoundingBox".into(),
        json!({"x":0,"y":0,"width":24,"height":24}),
    );
    data.assets.push(
        devup_mcp_figma::exported_asset_from_bytes(
            &devup_mcp_figma::AssetRequest {
                usage_aspect_ratio: None,
                max_width: None,
                srcset: false,
                visible_scope_node_id: None,
                render_mode: devup_mcp_figma::AssetRenderMode::Node,
                quality: None,
                lossless: None,
                asset_id: "1:1:node".into(),
                node_id: "1:1".into(),
                field: "node".into(),
                image_hash: None,
                format: devup_mcp_figma::AssetFormat::Svg,
                scale: 1.0,
            },
            b"<svg/>",
        )
        .unwrap(),
    );
    let result = project(data, operation(&["tsx", "assetManifest"]))
        .await
        .unwrap();
    assert!(result["tsx"].is_string());
    assert_eq!(result["failures"], json!([]));
    assert_eq!(result["assetSummary"]["status"], "collected");
}

#[tokio::test]
async fn r1_shared_visible_asset_path_is_not_excluded() {
    let mut data = payload();
    data.snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .fields
        .insert("childrenIds".into(), json!(["2:1", "2:2", "2:3"]));
    for (id, visible) in [("2:1", false), ("2:2", true)] {
        data.snapshot.nodes.insert(id.into(), serde_json::from_value(json!({"id":id,"type":"VECTOR",
                "fields":{"name":"Shared", "parentId":"1:1", "visible":visible,"width":24,"height":24}})).unwrap());
    }
    data.snapshot.nodes.insert(
        "2:3".into(),
        serde_json::from_value(
            json!({"id":"2:3", "type":"TEXT", "fields":{"parentId":"1:1", "characters":"Label"}}),
        )
        .unwrap(),
    );
    let mut op = operation(&["tsx", "assetManifest"]);
    if let PendingOperation::Export {
        asset_names_per_node,
        ..
    } = &mut op
    {
        *asset_names_per_node = false;
    }
    let result = project(data, op).await.unwrap();
    assert!(result["tsx"].is_string());
    assert_eq!(result["failures"], json!([]));
    let path = result["assetManifest"]["assets"][0]["path"]
        .as_str()
        .unwrap();
    assert!(result["tsx"].as_str().unwrap().contains(path));
}

fn payload() -> CollectedPayload {
    serde_json::from_value(json!({
        "target": {"fileKey":"W1Fixture", "nodeId":"1:1", "branchKey":null},
        "scope":"node", "metadata":{}, "variables":null, "styles":null,
        "completeness":"resolved-values-only", "sourceVersion":null,
        "snapshot": {"fileKey":"W1Fixture", "version":null, "roots":["1:1"],
            "nodes":{"1:1":{"id":"1:1","type":"FRAME","fields":{
                "name":"Screen", "x":0,"y":0,"width":390,"height":800,
                "visible":true,"childrenIds":[]}}},"diagnostics":[]}
    }))
    .unwrap()
}

#[tokio::test]
async fn p1_frames_report_generated_asset_only_equivalence() {
    let mut data = section(3);
    for i in 1..=3 {
        let id = format!("1:{i}");
        let node = data.snapshot.nodes.get_mut(&id).unwrap();
        node.fields.insert(
            "fills".into(),
            json!([{"type":"IMAGE", "imageHash":format!("image{i}"), "scaleMode":"FILL"}]),
        );
    }
    data.snapshot
        .nodes
        .get_mut("1:3")
        .unwrap()
        .fields
        .insert("childrenIds".into(), json!(["3:1"]));
    data.snapshot.nodes.insert(
        "3:1".into(),
        serde_json::from_value(json!({
            "id":"3:1", "type":"TEXT", "fields":{"name":"Copy", "parentId":"1:3",
                "characters":"Different copy", "fontSize":16, "width":100, "height":24}
        }))
        .unwrap(),
    );
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export { all_screens, .. } = &mut op {
        *all_screens = true;
    }
    let result = project(data, op).await.unwrap();
    assert_eq!(result["frames"][1]["structurallyIdenticalTo"], "1:1");
    assert_eq!(
        result["frames"][1]["differsOnly"],
        json!(["assetReferences"])
    );
    assert!(result["frames"][2].get("structurallyIdenticalTo").is_none());
}

#[tokio::test]
async fn p1_asset_summary_distinguishes_none_uncollected_and_hidden() {
    let result = project(payload(), operation(&["tsx"])).await.unwrap();
    assert_eq!(result["assetSummary"]["status"], "none");
    let mut data = payload();
    data.snapshot.nodes.get_mut("1:1").unwrap().node_type = "VECTOR".into();
    let result = project(data.clone(), operation(&["assetManifest"]))
        .await
        .unwrap();
    assert_eq!(result["assetSummary"]["status"], "not-collected");
    assert_eq!(
        result["assetSummary"]["unavailable"][0]["reason"],
        "not-requested"
    );
    data.snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .fields
        .insert("visible".into(), json!(false));
    let result = project(data, operation(&["assetManifest"])).await.unwrap();
    assert_eq!(result["assetSummary"]["status"], "not-collected");
    assert_eq!(
        result["assetSummary"]["unavailable"][0]["reason"],
        "hidden-node"
    );
}

#[tokio::test]
async fn p1_incomplete_asset_discovery_never_claims_no_assets() {
    let mut data = payload();
    data.snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .fields
        .insert("childrenIds".into(), json!(["missing"]));
    let result = project(data, operation(&["assetManifest"])).await.unwrap();
    assert_eq!(result["assetSummary"]["status"], "unknown");
    assert_eq!(result["assetSummary"]["discovery"], "incomplete");
}

#[tokio::test]
async fn p1_selection_separates_explicit_cases_from_automatic_screens() {
    let mut data = section(2);
    data.metadata["sectionIndex"]["candidates"][1]["bounds"]["width"] = json!(150);
    data.metadata["sectionIndex"]["candidates"][1]["bounds"]["height"] = json!(150);
    let result = project(data.clone(), operation(&["tsx"])).await.unwrap();
    assert_eq!(result["selection"]["count"], 1);
    assert_eq!(
        result["selection"]["explicitCandidates"][0]["node"]["nodeId"],
        "1:2"
    );
    assert_eq!(
        result["selection"]["explicitCandidates"][0]["node"]["kind"],
        "annotation"
    );
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export { frame_ids, .. } = &mut op {
        *frame_ids = vec!["1:2".into()];
    }
    let result = project(data, op).await.unwrap();
    assert_eq!(result["frames"][0]["nodeId"], "1:2");
}

#[test]
fn p1_asset_comparison_preserves_literal_text_and_unknown_paths() {
    let paths = vec!["/icons/a.svg".into(), "/icons/b.svg".into()];
    assert_eq!(
        generated_structure("<Box maskImage=\"url('/icons/a.svg')\" />", &paths),
        generated_structure("<Box maskImage=\"url('/icons/b.svg')\" />", &paths)
    );
    assert_ne!(
        generated_structure("<Text>{\"url(/icons/a.svg)\"}</Text>", &paths),
        generated_structure("<Text>{\"url(/icons/b.svg)\"}</Text>", &paths)
    );
    assert_ne!(
        generated_structure("<Image src=\"/icons/a.svg\" />", &paths),
        generated_structure("<Image src=\"/icons/unknown.svg\" />", &paths)
    );
}

#[test]
fn p1_layered_backgrounds_compare_every_asset_reference() {
    let paths = vec!["/images/a.png".into(), "/images/b.png".into()];
    let first = "<Box bg=\"linear-gradient(red, blue), url('/images/a.png') center/cover, url(/images/b.png)\" />";
    let second = "<Box bg=\"linear-gradient(red, blue), url('/images/b.png') center/cover, url(/images/a.png)\" />";
    assert_eq!(
        generated_structure(first, &paths),
        generated_structure(second, &paths)
    );
    assert_ne!(
        generated_structure(first, &paths),
        generated_structure(&second.replace("red", "green"), &paths)
    );
}

#[tokio::test]
async fn p1_selection_does_not_claim_to_include_an_unproduced_manifest() {
    let result = project(section(2), operation(&["tsx", "assetManifest"]))
        .await
        .unwrap();
    assert_eq!(result["status"], "selection_required");
    assert!(result.get("assetManifest").is_none());
    assert_eq!(result["assetSummary"]["manifestIncluded"], false);
    for output in ["imageAudit", "assetManifest"] {
        let result = project(section(2), operation(&[output])).await.unwrap();
        assert_eq!(result["status"], "selection_required");
        assert!(
            result["nextAction"]["example"]["arguments"]["outputs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|o| o == output)
        );
    }
}

#[test]
fn p1_comparison_scopes_partial_frames_to_their_generated_outputs() {
    let mut frames = vec![
        json!({"nodeId":"1", "status":"complete", "tsx":"code", "componentTsx":"component"}),
        json!({"nodeId":"2", "status":"partial", "tsx":"code"}),
        json!({"nodeId":"3", "status":"partial", "tsx":"code", "componentTsx":"component"}),
        json!({"nodeId":"4", "status":"partial"}),
    ];
    annotate_identical_frames(&mut frames, &payload(), true);
    assert!(frames[1].get("structurallyIdenticalTo").is_none());
    assert_eq!(frames[2]["structurallyIdenticalTo"], "1");
    assert_eq!(frames[2]["comparedOutputs"], json!(["tsx", "componentTsx"]));
    assert!(frames[3].get("structurallyIdenticalTo").is_none());
}

#[tokio::test]
async fn p1_identical_hints_and_asset_summary_survive_resource_delivery() {
    let mut op = operation(&["tsx", "componentTsx"]);
    if let PendingOperation::Export {
        all_screens,
        delivery,
        ..
    } = &mut op
    {
        *all_screens = true;
        *delivery = DeliveryMode::Resource;
    }
    let result = project(section(2), op).await.unwrap();
    assert!(result["frames"][0].get("tsx").is_none());
    assert_eq!(result["frames"][1]["structurallyIdenticalTo"], "1:1");
    assert_eq!(result["frames"][1]["differsOnly"], json!([]));
    assert_eq!(
        result["frames"][1]["comparedOutputs"],
        json!(["tsx", "componentTsx"])
    );
    assert!(result["assetSummary"].is_object());
    assert!(result["frames"][0]["assetSummary"].is_object());
}

#[tokio::test]
async fn p1_collected_assets_are_distinct_from_delivered_manifest() {
    let mut data = payload();
    data.snapshot.nodes.get_mut("1:1").unwrap().node_type = "VECTOR".into();
    data.assets.push(
        devup_mcp_figma::exported_asset_from_bytes(
            &devup_mcp_figma::AssetRequest {
                usage_aspect_ratio: None,
                max_width: None,
                srcset: false,
                visible_scope_node_id: None,
                render_mode: devup_mcp_figma::AssetRenderMode::Node,
                quality: None,
                lossless: None,
                asset_id: "1:1:node".into(),
                node_id: "1:1".into(),
                field: "node".into(),
                image_hash: None,
                format: devup_mcp_figma::AssetFormat::Svg,
                scale: 1.0,
            },
            b"<svg/>",
        )
        .unwrap(),
    );
    let result = project(data, operation(&["tsx"])).await.unwrap();
    assert_eq!(result["assetSummary"]["status"], "collected");
    assert_eq!(result["assetSummary"]["collectedCount"], 1);
    assert_eq!(result["assetSummary"]["manifestIncluded"], false);
    assert!(result.get("assetManifest").is_none());
}

#[tokio::test]
async fn p3_identical_bytes_share_manifest_paths() {
    let mut data = payload();
    for (id, path) in [("1:2", "/icons/a.svg"), ("1:3", "/icons/b.svg")] {
        data.assets.push(
            serde_json::from_value(json!({
                "assetId":id,"nodeId":id,"field":"svg","sourceKind":"svg",
                "status":"exported","format":"svg","scale":1,"mimeType":"image/svg+xml",
                "dataBase64":STANDARD.encode(b"<svg/>"), "byteLength":6,
                "sha256":sha256_hex(b"<svg/>"),"path":path
            }))
            .unwrap(),
        );
    }
    let result = project(data, operation(&["assetManifest"])).await.unwrap();
    let assets = result["assetManifest"]["assets"].as_array().unwrap();
    assert_eq!(assets[0]["path"], assets[1]["path"]);
}

#[tokio::test]
async fn p3_selection_call_preserves_export_options() {
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export {
        component_name,
        strict,
        ..
    } = &mut op
    {
        *component_name = Some("TicketScreen".into());
        *strict = true;
    }
    let result = project(section(2), op).await.unwrap();
    let args = &result["nextAction"]["example"]["arguments"];
    assert_eq!(args["componentName"], "TicketScreen");
    assert_eq!(args["strict"], true);
    assert_eq!(args["frameIds"], json!(["1:1"]));
}

fn original_source_payload() -> CollectedPayload {
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(800, 500, |x, y| {
        image::Rgba([(x % 255) as u8, (y % 255) as u8, 127, 179])
    }))
    .write_to(&mut png, image::ImageFormat::Png)
    .unwrap();
    let mut data = p3_asset_payload(png.get_ref());
    data.assets.clear();
    data.snapshot.nodes.remove("1:3");
    data.snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .fields
        .insert("childrenIds".into(), json!(["1:2"]));
    let node = data.snapshot.nodes.get_mut("1:2").unwrap();
    node.fields.insert("width".into(), json!(120.1));
    node.fields.insert("height".into(), json!(80));
    node.fields.insert(
        "effects".into(),
        json!([{"type":"LAYER_BLUR","radius":12,"visible":true}]),
    );
    let req = devup_mcp_figma::AssetRequest {
        usage_aspect_ratio: Some(1.0),
        max_width: Some(300),
        srcset: true,
        visible_scope_node_id: None,
        quality: Some(85),
        lossless: Some(true),
        render_mode: devup_mcp_figma::AssetRenderMode::OriginalSource,
        asset_id: "1:2:fills:0".into(),
        node_id: "1:2".into(),
        field: "fills/0".into(),
        image_hash: Some("1:2".into()),
        format: devup_mcp_figma::AssetFormat::Webp,
        scale: 2.0,
    };
    let result = devup_mcp_figma::UpstreamResult {
        raw: json!({"kind":"devupAssetExport","fileKey":"W1Fixture","version":null,"assetId":req.asset_id,"nodeId":req.node_id,"field":req.field,"imageHash":req.image_hash,"format":"webp","scale":2,"renderMode":"original-source","usageAspectRatio":1,"maxWidth":300,"srcset":true,"visibleScopeNodeId":null,"status":"exported","mimeType":"image/png","byteLength":png.get_ref().len(),"sha256":sha256_hex(png.get_ref()),"data":STANDARD.encode(png.get_ref()),"exportDiagnostics":{"method":"original-fill","stages":[],"renderInfo":{"representation":"original-fill","nodeSize":{"width":120.1,"height":80,"aspectRatio":120.1/80.0},"sourceProperties":{"scaleMode":"FILL","rotation":0,"effects":[{"type":"LAYER_BLUR","radius":12}]}}}}),
    };
    let devup_mcp_figma::AssetExportOutcome::Entry(entry) =
        devup_mcp_figma::asset_export_from_result(&result, "W1Fixture", None, &req).unwrap()
    else {
        panic!("entry")
    };
    data.assets.push(*entry);
    data
}

#[tokio::test]
async fn original_source_variants_css_and_public_files_share_actual_byte_paths() {
    let root =
        std::env::temp_dir().join(format!("devup-original-public-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&root).unwrap();
    let root = crate::test_paths::canonical(&root);
    let base = root.join("images").join("source.webp");
    let css = root.join("asset-css-map.json");
    let mut op = operation(&["tsx", "assetManifest", "assetCssMap", "imageAudit"]);
    if let PendingOperation::Export {
        asset_output_paths,
        asset_public_root,
        output_paths,
        ..
    } = &mut op
    {
        asset_output_paths.insert("1:2:fills:0".into(), base.to_string_lossy().into());
        *asset_public_root = Some(root.clone());
        output_paths.insert("assetCssMap".into(), css.to_string_lossy().into());
    }
    let value = project(original_source_payload(), op).await.unwrap();
    let asset = &value["assetManifest"]["assets"][0];
    let path = asset["path"].as_str().unwrap();
    assert_eq!(path, "/images/source.webp");
    assert_eq!(asset["variants"][0]["path"], "/images/source@2x.webp");
    let base_image = image::open(&base).unwrap();
    let twice = image::open(root.join("images/source@2x.webp")).unwrap();
    assert_eq!(base_image.width(), 121);
    assert_eq!(twice.width(), 241);
    let code = value["tsx"].as_str().unwrap();
    assert!(code.contains(path));
    assert!(code.contains("/images/source@2x.webp"), "{code}");
    assert!(code.contains("blur(12px)"));
    assert!(code.contains("srcSet="));
    let map: Value = serde_json::from_slice(&std::fs::read(&css).unwrap()).unwrap();
    assert_eq!(
        std::fs::read(&css).unwrap(),
        serde_json::to_vec_pretty(&value["assetCssMap"]).unwrap(),
        "written CSS mapping must match the inline JSON encoding byte-for-byte"
    );
    assert_eq!(map["assets"][0]["effectsBaked"], false);
    assert_eq!(map["assets"][0]["assetPath"], path);
    assert_eq!(
        value["imageAudit"]["assets"][0]["aspectRatioComparison"]["nodeVsUsage"],
        "mismatch"
    );
    assert!(
        value["imageAudit"]["assets"][0]["aspectRatioComparison"]["usageEvidence"]
            .as_str()
            .unwrap()
            .contains("caller-declared")
    );
    std::fs::remove_dir_all(root).unwrap();
}
