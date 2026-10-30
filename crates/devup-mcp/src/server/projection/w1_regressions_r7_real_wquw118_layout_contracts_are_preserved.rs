
#[tokio::test]
async fn r7_real_wquw118_layout_contracts_are_preserved() {
    let data: CollectedPayload = serde_json::from_str(include_str!(
        "../../../../../fixtures/r2/wquw-118-payload.json"
    ))
    .unwrap();
    let mut op = operation(&["tsx", "sourceMap"]);
    if let PendingOperation::Export { frame_ids, .. } = &mut op {
        *frame_ids = vec!["3997:46277".into()];
    }
    let result = project(data, op).await.unwrap();
    assert_eq!(result["frames"].as_array().unwrap().len(), 1);
    let frame = &result["frames"][0];
    assert_eq!(frame["nodeId"], "3997:46277");
    let entries = frame["sourceMap"]["entries"].as_array().unwrap();
    for (id, field, resolution) in [
        ("3997:46293", "width", "accounted-for-implicit-flex-grow"),
        ("3997:46295", "width", "accounted-for-implicit-flex-grow"),
        (
            "3997:46313",
            "layoutSizingVertical",
            "accounted-for-implicit-flex-stretch",
        ),
    ] {
        assert!(
            entries.iter().any(|e| e["nodeId"] == id
                && e["property"] == field
                && e["resolution"] == resolution),
            "{id}"
        );
    }
    assert!(
        frame["tsx"]
            .as_str()
            .unwrap()
            .contains("alignSelf=\"stretch\"")
    );
    for id in ["3997:46293", "3997:46295", "3997:46313"] {
        assert!(
            !result["projectionIssues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["nodeId"] == id && i["fidelityImpact"] == "lossy"),
            "{result}"
        );
    }
}

#[tokio::test]
async fn r7_artifact_reuses_stored_selection() {
    let data = section(2);
    let store = ArtifactStore::default();
    let mut request = CollectionRequest::new(data.target.clone(), CollectionScope::Node);
    request.section = Some(devup_mcp_figma::SectionReadOptions {
        frame_ids: vec!["1:2".into()],
        all_screens: false,
    });
    let artifact = store
        .insert(ArtifactRequestKey::from_collection(&request), data)
        .await
        .unwrap();
    let result = complete_operation(
        operation(&["tsx"]),
        &artifact.payload,
        "artifact",
        &artifact,
        &OutputPolicy::from_roots(vec![std::env::temp_dir()]).unwrap(),
        &store,
    )
    .await
    .unwrap();
    assert_ne!(result["status"], "selection_required");
    assert_eq!(result["frames"][0]["nodeId"], "1:2");
    assert_eq!(result["frames"].as_array().unwrap().len(), 1);
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export { frame_ids, .. } = &mut op {
        *frame_ids = vec!["1:1".into()];
    }
    let explicit = complete_operation(
        op,
        &artifact.payload,
        "artifact",
        &artifact,
        &OutputPolicy::from_roots(vec![std::env::temp_dir()]).unwrap(),
        &store,
    )
    .await
    .unwrap();
    assert_eq!(explicit["frames"][0]["nodeId"], "1:1");
}

#[tokio::test]
async fn r7_auto_debug_resource_has_inline_reprojection_guidance() {
    let mut data = payload();
    data.metadata = json!({"large":"x".repeat(300_000)});
    let mut op = operation(&["tsx", "rawPayload", "sourceMap"]);
    if let PendingOperation::Export { delivery, .. } = &mut op {
        *delivery = DeliveryMode::Auto;
    }
    let result = project(data, op).await.unwrap();
    assert!(result["resources"].is_array());
    assert_eq!(
        result["nextAction"]["example"]["arguments"]["delivery"],
        "inline"
    );
    assert_eq!(result["nextAction"]["example"]["arguments"]["debug"], true);
    assert!(
        result["nextAction"]["how"]
            .as_str()
            .unwrap()
            .contains("resource")
    );
}

#[tokio::test]
async fn r12_oversized_export_retains_artifact_and_callable_resource_projection() {
    use super::super::{DevupAuth, DevupServer, Services};
    use devup_mcp_figma::{AuthStatus, FigmaUpstream, ReadToolCall, UpstreamResult};
    use std::sync::Arc;
    struct Offline;
    #[async_trait::async_trait]
    impl DevupAuth for Offline {
        async fn status(&self) -> Result<AuthStatus, DevupError> {
            panic!("cached projection must not authenticate")
        }
        async fn login(&self) -> Result<AuthStatus, DevupError> {
            panic!("cached projection must not login")
        }
        async fn logout(&self) -> Result<AuthStatus, DevupError> {
            panic!("cached projection must not logout")
        }
    }
    #[async_trait::async_trait]
    impl FigmaUpstream for Offline {
        async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
            panic!("cached projection must not list upstream tools")
        }
        async fn call_read_tool(&self, _: ReadToolCall) -> Result<UpstreamResult, DevupError> {
            panic!("cached projection must not call Figma")
        }
    }
    let mut data = payload();
    data.metadata = json!({"large":"x".repeat(600_000)});
    let store = ArtifactStore::default();
    let key = ArtifactRequestKey::from_collection(&CollectionRequest::new(
        data.target.clone(),
        CollectionScope::Node,
    ));
    let artifact = store.insert(key, data).await.unwrap();
    let policy = OutputPolicy::from_roots(vec![std::env::temp_dir()]).unwrap();
    let outputs = ["tsx", "rawPayload", "sourceMap", "assetManifest"];
    let error = complete_operation(
        operation(&outputs),
        &artifact.payload,
        "artifact",
        &artifact,
        &policy,
        &store,
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupFigmaResponseTooLarge);
    let envelope = super::super::structured_tool_error(super::super::to_mcp_error(error.clone()));
    let wire = serde_json::to_value(&envelope).unwrap();
    assert_eq!(wire["isError"], true);
    assert_eq!(
        serde_json::from_str::<Value>(wire["content"][0]["text"].as_str().unwrap()).unwrap(),
        wire["structuredContent"]
    );
    assert_eq!(wire["structuredContent"]["error"]["details"], error.details);
    let details = &error.details;
    assert_eq!(details["recoveryState"], "available");
    assert_eq!(details["nextAction"]["tool"], "devup_figma_export");
    let args = &details["nextAction"]["arguments"];
    assert_eq!(args["artifactId"], artifact.artifact_id);
    assert_eq!(args["delivery"], "resource");
    assert_eq!(args["outputs"], json!(outputs));
    assert_eq!(args["debug"], true);
    assert!(details["serializedResponseBytes"].as_u64().unwrap() > 1_048_576);
    assert!(store.get(&artifact.artifact_id).await.is_some());
    let mut calls = vec![args.clone()];
    for next in details["splitRequests"].as_array().unwrap() {
        assert_eq!(next["tool"], "devup_figma_export");
        calls.push(next["arguments"].clone());
    }
    assert!(calls.len() > 1);
    let mut server = DevupServer::new(Services::new(Arc::new(Offline), Arc::new(Offline)));
    server.artifacts = store.clone();
    for args in calls {
        // Deserialize and execute the complete callable arguments through
        // the actual tool handler, including its validation contract.
        let result = server
            .devup_figma_export(rmcp::handler::server::wrapper::Parameters(
                serde_json::from_value(args).unwrap(),
            ))
            .await
            .unwrap()
            .structured_content
            .unwrap();
        assert!(result["resources"].is_array());
        assert_eq!(result["cache"]["artifactId"], artifact.artifact_id);
    }
}

#[test]
fn r8_nonsection_semantic_asset_mapping_uses_delivered_path() {
    let mut result = json!({"tsx":"<Image src=\"/icons/grommet-icons:language.svg\" />", "sourceMap":{"tsx":[{"nodeId":"한글:1","property":"fills","generatedProperty":"src=\"/icons/grommet-icons:language.svg\"","resolution":"asset"}]}});
    rewrite_result_asset_references(&mut result, &BTreeMap::new());
    assert_eq!(
        result["sourceMap"]["tsx"][0]["generatedProperty"],
        "src=\"/icons/grommet-icons-language-d201f62b16f5.svg\""
    );
}

#[tokio::test]
async fn r8_three_frame_nine_output_units_measure_local_projection() {
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
    assert_eq!(outputs.len(), 8); // shared snapshot/audit plus 3 TSX/sourceMap pairs
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
        "R8 offline 3-frame/9-unit projection: {:?}; no upstream timing inferred",
        began.elapsed()
    );
}

#[tokio::test]
async fn r8_final_korean_semantic_map_follows_asset_path_rewrites() {
    let mut data: CollectedPayload = serde_json::from_str(include_str!(
        "../../../../../fixtures/r2/wquw-120-payload.json"
    ))
    .unwrap();
    data.snapshot = serde_json::from_str(include_str!(
        "../../../../../fixtures/r8/modal-snapshot.json"
    ))
    .unwrap();
    let mut op = operation(&["tsx", "sourceMap"]);
    if let PendingOperation::Export { frame_ids, .. } = &mut op {
        *frame_ids = vec!["3997:46582".into()];
    }
    let started = std::time::Instant::now();
    let result = project(data, op).await.unwrap();
    eprintln!(
        "R8 modal projection: {:?}; executable: {:?}",
        started.elapsed(),
        std::env::current_exe().unwrap()
    );
    let frame = &result["frames"][0];
    let tsx = frame["tsx"].as_str().unwrap();
    assert!(tsx.contains("추가 체험"));
    let entries = frame["sourceMap"]["entries"].as_array().unwrap();
    for entry in entries {
        assert!(
            entry.get("generatedRange").is_none(),
            "public offsets must not return: {entry}"
        );
        assert!(entry["nodeId"].is_string() && entry["property"].is_string());
        let generated = entry["generatedProperty"]
            .as_str()
            .expect("mapped property required");
        assert!(!generated.is_empty());
        if !generated.starts_with("implicit:") && generated != "children" {
            assert!(
                tsx.contains(generated),
                "mapping must name emitted property: {entry}"
            );
        }
    }
    let modal_type = entries
        .iter()
        .find(|e| e["nodeId"] == "3997:46621" && e["property"] == "type")
        .unwrap();
    assert_eq!(modal_type["generatedProperty"], "Box");
    assert_eq!(modal_type["resolution"], "exact");
    assert_eq!(frame["sourceMap"]["source"]["generatedOutput"], "tsx");
    eprintln!(
        "R8 sourceMap={} bytes, TSX={} bytes",
        serde_json::to_vec(&frame["sourceMap"]).unwrap().len(),
        tsx.len()
    );
}

#[tokio::test]
async fn r16_verdict_scope_identifies_remaining_asset_axes_without_diagnostics() {
    let mut data: CollectedPayload = serde_json::from_str(include_str!(
        "../../../../../fixtures/r2/wquw-118-payload.json"
    ))
    .unwrap();
    data.snapshot = serde_json::from_str(include_str!(
        "../../../../../fixtures/r14/asset-snapshot.json"
    ))
    .unwrap();
    let mut op = operation(&["tsx", "sourceMap"]);
    if let PendingOperation::Export {
        frame_ids,
        include_diagnostics,
        ..
    } = &mut op
    {
        *frame_ids = vec!["3997:46667".into()];
        *include_diagnostics = false;
    }
    let result = project(data, op).await.unwrap();
    assert_eq!(result["status"], "partial");
    assert_eq!(result["quality"]["projection"], "approximated");
    for owner in [&result, &result["frames"][0]] {
        let scope = &owner["verdictScope"];
        assert_eq!(scope["status"], "partial");
        assert_eq!(scope["projection"], "approximated");
        let causes: Vec<_> = scope["projectionCauses"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| {
                c["nodeId"] == "3997:46668" && c["code"] == "DEVUP_CODEGEN_ABSOLUTE_FALLBACK"
            })
            .collect();
        assert_eq!(causes.len(), 2);
        assert_eq!(causes[0]["component"], "horizontal");
        assert_eq!(causes[1]["component"], "vertical");
        for cause in causes {
            assert_eq!(cause["screenId"], "3997:46667");
            assert_eq!(cause["state"], "approximated");
            assert!(cause["reason"].as_str().is_some_and(|s| !s.is_empty()));
        }
    }
}

#[tokio::test]
async fn r16_complete_scope_and_mapping_semantics_survive_resource_delivery() {
    for (delivery_mode, real_fill) in [
        (DeliveryMode::Inline, false),
        (DeliveryMode::Resource, false),
        (DeliveryMode::Inline, true),
        (DeliveryMode::Resource, true),
    ] {
        let data: CollectedPayload = if real_fill {
            serde_json::from_str(include_str!(
                "../../../../../fixtures/r2/wquw-118-payload.json"
            ))
            .unwrap()
        } else {
            let mut data = section(1);
            data.snapshot.nodes.get_mut("1:1").unwrap().fields.extend(
                    serde_json::from_value::<BTreeMap<String, Value>>(json!({"layoutSizingHorizontal":"FIXED", "layoutSizingVertical":"FIXED", "fills":[{"type":"SOLID","color":{"r":0.2,"g":0.4,"b":0.6}}]})).unwrap()
                );
            data
        };
        let mut op = operation(&["tsx", "sourceMap"]);
        if let PendingOperation::Export {
            frame_ids,
            delivery,
            ..
        } = &mut op
        {
            *frame_ids = vec![if real_fill { "3997:46129" } else { "1:1" }.into()];
            *delivery = delivery_mode;
        }
        let result = project(data, op).await.unwrap();
        if real_fill {
            assert_eq!(result["status"], "partial");
            assert_eq!(result["verdictScope"]["projection"], "lossy");
            let fill_issues: Vec<_> = result["projectionIssues"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|d| d["property"] == "layoutSizingHorizontal")
                .collect();
            assert_eq!(fill_issues.len(), 18);
            for issue in fill_issues {
                assert_eq!(issue["fidelityImpact"], "lossy");
                assert_eq!(issue["details"]["originalValue"], "FILL");
            }
            assert_eq!(
                result["frames"][0]["outputResults"]["tsx"]["mappingComplete"],
                false
            );
        } else {
            assert_eq!(result["status"], "complete");
            assert_eq!(result["verdictScope"]["projectionCauses"], json!([]));
            assert_eq!(result["verdictScope"]["statusCauses"], json!([]));
            assert_eq!(result["verdictScope"]["projection"], "exact");
        }
        assert_eq!(result["resolutionSemantics"]["axis"], "mapping-method");
        assert!(
            result["resolutionSemantics"]["values"]["raw-fallback"]
                .as_str()
                .unwrap()
                .contains("verified")
        );
        if result.get("resources").is_none() {
            assert_eq!(
                result["frames"][0]["sourceMap"]["resolutionSemantics"]["axis"],
                "mapping-method"
            );
        }
    }
}

#[tokio::test]
async fn fields_narrow_raw_snapshot_and_source_map() {
    let mut op = operation(&["tsx", "rawSnapshot", "sourceMap"]);
    if let PendingOperation::Export { fields, .. } = &mut op {
        *fields = vec!["typography".into()];
    }
    let result = project(payload(), op).await.unwrap();
    for node in result["rawSnapshot"]["nodes"].as_object().unwrap().values() {
        for key in node["fields"].as_object().unwrap().keys() {
            assert!(
                [
                    "fontSize",
                    "fontName",
                    "fontWeight",
                    "lineHeight",
                    "letterSpacing",
                    "textStyleId",
                    "styledTextSegments",
                    "textAlignHorizontal",
                    "characters",
                    "childrenIds",
                    "parentId",
                    "name",
                    "visible",
                    "isAsset"
                ]
                .contains(&key.as_str()),
                "unselected field survived: {key}"
            );
        }
    }
    for entry in result["sourceMap"]["tsx"].as_array().into_iter().flatten() {
        assert!(entry["property"] != "width", "{entry}");
    }
}
#[tokio::test]
async fn r16_non_projection_partial_has_its_own_status_cause() {
    let mut data = payload();
    data.snapshot
        .nodes
        .get_mut("1:1")
        .unwrap()
        .field_errors
        .insert("name".into(), "unavailable".into());
    let result = project(data, operation(&["rawSnapshot"])).await.unwrap();
    assert_eq!(result["status"], "partial");
    assert_eq!(result["quality"]["projection"], "not-requested");
    assert_eq!(result["verdictScope"]["projectionCauses"], json!([]));
    assert!(
        result["verdictScope"]["statusCauses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["domain"] == "acquisition" && c["state"] == "partial")
    );
    let acquisition = result["verdictScope"]["statusCauses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["domain"] == "acquisition")
        .unwrap();
    assert_eq!(acquisition["evidenceScope"], "response");
}

#[tokio::test]
async fn r16_frame_scope_retains_scoped_output_failures() {
    let mut data = section(2);
    // Unlike the deliberately underspecified generic Section fixture,
    // this successful sibling has a complete fixed-dimension proof.
    data.snapshot.nodes.get_mut("1:1").unwrap().fields.extend(
        serde_json::from_value::<BTreeMap<String, Value>>(json!({
            "layoutSizingHorizontal":"FIXED", "layoutSizingVertical":"FIXED"
        }))
        .unwrap(),
    );
    data.snapshot.nodes.remove("1:2");
    data.snapshot.roots = vec!["1:1".into()];
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export { frame_ids, .. } = &mut op {
        *frame_ids = vec!["1:1".into(), "1:2".into()];
    }
    let result = project(data, op).await.unwrap();
    let good = &result["frames"][0];
    let failed = &result["frames"][1];
    assert_eq!(good["verdictScope"]["statusCauses"], json!([]));
    let causes = failed["verdictScope"]["statusCauses"].as_array().unwrap();
    assert!(causes.iter().any(|c| c["domain"] == "requested-output"
        && c["nodeId"] == "1:2"
        && c["output"] == "tsx"));
    for cause in causes.iter().filter(|c| c["domain"] == "requested-output") {
        let i = cause["failureIndex"].as_u64().unwrap() as usize;
        assert_eq!(failed["failures"][i]["errorCode"], cause["code"]);
    }
}

#[tokio::test]
async fn r16_resource_scope_retains_every_projection_issue() {
    for asset_case in [false, true] {
        let mut data: CollectedPayload = serde_json::from_str(include_str!(
            "../../../../../fixtures/r2/wquw-119-payload.json"
        ))
        .unwrap();
        let selected = if asset_case {
            data.snapshot = serde_json::from_str(include_str!(
                "../../../../../fixtures/r14/asset-snapshot.json"
            ))
            .unwrap();
            vec!["3997:46667".into()]
        } else {
            vec![
                "3997:46315".into(),
                "3997:46715".into(),
                "3997:46333".into(),
            ]
        };
        let mut op = operation(&["tsx", "rawSnapshot", "sourceMap"]);
        if let PendingOperation::Export {
            frame_ids,
            delivery,
            ..
        } = &mut op
        {
            *frame_ids = selected;
            *delivery = DeliveryMode::Resource;
        }
        let result = project(data, op).await.unwrap();
        assert_eq!(result["resolutionSemantics"]["axis"], "mapping-method");
        if asset_case {
            assert!(!result["projectionIssues"].as_array().unwrap().is_empty());
        }
        for owner in std::iter::once(&result).chain(result["frames"].as_array().unwrap()) {
            let causes = owner["verdictScope"]["projectionCauses"]
                .as_array()
                .unwrap();
            let issues = owner["projectionIssues"].as_array().unwrap();
            for (i, issue) in issues.iter().enumerate() {
                assert!(causes.iter().any(|c| c["diagnosticIndex"] == i
                    && c["nodeId"] == issue["nodeId"]
                    && c["code"] == issue["code"]));
            }
            eprintln!(
                "R16 metadata node={} scopeBytes={} issues={}",
                owner["nodeId"],
                serde_json::to_vec(&owner["verdictScope"]).unwrap().len(),
                issues.len()
            );
        }
    }
}

include!("../projection_fingerprint_tests.rs");

#[tokio::test]
async fn feedback_interaction_hints_match_saved_files_and_resource_hashes() {
    for section_case in [false, true] {
        let mut baseline_op = operation(&["tsx", "sourceMap"]);
        if section_case && let PendingOperation::Export { frame_ids, .. } = &mut baseline_op {
            *frame_ids = vec!["1:1".into()];
        }
        let baseline = project(
            if section_case { section(2) } else { payload() },
            baseline_op,
        )
        .await
        .unwrap();
        let mut data = if section_case { section(2) } else { payload() };
        let node = data.snapshot.nodes.get_mut("1:1").unwrap();
        node.fields.insert("name".into(), json!("CTA Button"));
        node.fields
            .insert("variantProperties".into(), json!({"State":"disabled"}));
        node.fields.insert("reactions".into(),json!([{"trigger":{"type":"ON_CLICK"},"actions":[{"type":"URL","url":"https://example.com/confirmed"}]}]));
        let target = std::env::temp_dir().join(format!(
            "feedback-hints-{}-{}.tsx",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut op = operation(&["tsx", "sourceMap"]);
        if let PendingOperation::Export {
            delivery,
            output_paths,
            frame_ids,
            ..
        } = &mut op
        {
            *delivery = DeliveryMode::Resource;
            if section_case {
                *frame_ids = vec!["1:1".into()];
            }
            output_paths.insert(
                if section_case { "frame:1:1:tsx" } else { "tsx" }.into(),
                target.to_string_lossy().into(),
            );
        }
        let store = ArtifactStore::default();
        let key = ArtifactRequestKey::from_collection(&CollectionRequest::new(
            data.target.clone(),
            CollectionScope::Node,
        ));
        let artifact = store.insert(key, data).await.unwrap();
        let result = complete_operation(
            op,
            &artifact.payload,
            "fixture",
            &artifact,
            &OutputPolicy::from_roots(vec![std::env::temp_dir()]).unwrap(),
            &store,
        )
        .await
        .unwrap();
        let manifest = store
            .output_manifests()
            .await
            .into_iter()
            .find(|m| m.mime_type == "text/typescript")
            .unwrap();
        let mut bytes = vec![];
        for index in 0..manifest.chunk_count {
            bytes.extend(
                store
                    .read_output_chunk(&manifest.artifact_id, &manifest.output_id, index)
                    .await
                    .unwrap(),
            );
        }
        assert_eq!(bytes, std::fs::read(&target).unwrap());
        assert_eq!(bytes.len(), manifest.raw_bytes);
        assert_eq!(sha256_hex(&bytes), manifest.sha256);
        let code = String::from_utf8(bytes).unwrap();
        assert!(code.contains("// @devup-interaction "));
        assert!(code.contains("https://example.com/confirmed"));
        assert!(code.contains("disabled"));
        assert!(!code.contains("href="));
        assert_eq!(result["deliverable"]["isFinal"], false);
        assert_eq!(result["mappingComplete"], baseline["mappingComplete"]);
        assert!(
            result["interactiveCandidates"]
                .as_array()
                .is_some_and(|a| !a.is_empty())
        );
        std::fs::remove_file(target).unwrap();
    }
}

async fn project(
    payload: CollectedPayload,
    operation: PendingOperation,
) -> Result<Value, DevupError> {
    let store = ArtifactStore::default();
    let key = ArtifactRequestKey::from_collection(&CollectionRequest::new(
        payload.target.clone(),
        CollectionScope::Node,
    ));
    let artifact = store.insert(key, payload).await.unwrap();
    complete_operation(
        operation,
        &artifact.payload,
        "fixture",
        &artifact,
        &OutputPolicy::from_roots(vec![std::env::temp_dir()]).unwrap(),
        &store,
    )
    .await
}

#[tokio::test]
async fn w1_component_tsx_projects_selected_frames_without_section_root() {
    let mut op = operation(&["componentTsx"]);
    if let PendingOperation::Export { frame_ids, .. } = &mut op {
        *frame_ids = vec!["1:1".into()];
    }
    let result = project(section(2), op).await.unwrap();
    assert_eq!(result["frames"].as_array().unwrap().len(), 1);
    assert!(
        result["frames"][0]["componentTsx"]
            .as_str()
            .unwrap()
            .contains("export function")
    );
    assert_ne!(result["quality"]["projection"], "not-requested");
}

#[tokio::test]
async fn w1_unavailable_responsive_output_is_actionable_even_with_tsx_and_strict() {
    for outputs in [vec!["responsiveTsx"], vec!["tsx", "responsiveTsx"]] {
        for strict_value in [false, true] {
            let mut op = operation(&outputs);
            if let PendingOperation::Export {
                all_screens,
                strict,
                ..
            } = &mut op
            {
                *all_screens = true;
                *strict = strict_value;
            }
            let response = project(section(2), op).await;
            if outputs == vec!["responsiveTsx"] {
                let error = response.unwrap_err();
                assert_eq!(error.details["nextAction"]["arguments"]["outputs"], json!(["tsx"]));
                assert_eq!(error.details["frameReasons"].as_array().unwrap().len(), 2);
                continue;
            }
            let result = if strict_value {
                response.unwrap_err().details
            } else {
                response.unwrap()
            };
            let failure = result["failures"].as_array().unwrap().iter()
                .find(|failure| failure["output"] == "responsiveTsx").unwrap();
            assert_eq!(failure["details"]["nextAction"]["tool"], "devup_figma_export");
            assert_eq!(failure["details"]["nextAction"]["arguments"]["outputs"], json!(["tsx"]));
            assert_eq!(failure["details"]["frameReasons"].as_array().unwrap().len(), 2);
            if !strict_value {
                assert_eq!(result["outputResults"]["responsiveTsx"]["state"], "failed");
            }
        }
    }
}

#[tokio::test]
async fn w1_asset_failure_is_partial_and_strict_refuses_it() {
    let mut data = payload();
    data.assets.push(serde_json::from_value(json!({"assetId":"1:1:fills:0","nodeId":"1:1",
            "field":"fills/0","sourceKind":"image","status":"failed","errorCode":"DEVUP_ASSET_EXPORT_FAILED"})).unwrap());
    let result = project(data.clone(), operation(&["assetManifest"]))
        .await
        .unwrap();
    assert_eq!(result["status"], "partial");
    assert_eq!(result["failures"][0]["assetId"], "1:1:fills:0");
    assert_ne!(result["quality"]["assets"], "complete");
    let mut strict_op = operation(&["assetManifest"]);
    if let PendingOperation::Export { strict, .. } = &mut strict_op {
        *strict = true;
    }
    assert!(project(data, strict_op).await.is_err());
}

#[tokio::test]
async fn nested_asset_failure_preserves_details_without_promising_a_path_or_retry() {
    for (code, detail, retryable) in [
        (
            "DEVUP_ASSET_EXPORT_FAILED",
            "export failed after 10400 ms: Unable to establish connection to Figma after 10 seconds",
            false,
        ),
        (
            "DEVUP_ASSET_EXPORT_FAILED",
            "native export: renderer failed; geometry fallback unsupported: mask requires native export",
            false,
        ),
        (
            "DEVUP_ASSET_TIMEOUT",
            "export exceeded 180000 ms asset budget",
            true,
        ),
    ] {
        let mut data = payload();
        data.assets.push(serde_json::from_value(json!({"assetId":"1:1:fills:0","nodeId":"1:1",
                "field":"fills/0","sourceKind":"image","status":"failed","errorCode":code,
                "errorDetail":detail,"path":"/icons/not-generated.svg","outputPath":"C:/not-generated.svg"})).unwrap());
        let result = project(data, operation(&["assetManifest"])).await.unwrap();
        assert_eq!(result["status"], "partial");
        assert_eq!(result["failures"][0]["retryable"], retryable);
        assert_eq!(result["failures"][0]["errorDetail"], detail);
        let asset = &result["assetManifest"]["assets"][0];
        assert_eq!(asset["status"], "failed");
        assert!(asset["path"].is_null() && asset["outputPath"].is_null());
        assert_eq!(
            result["assetSummary"]["unavailable"][0]["errorDetail"],
            detail
        );
        assert!(result["outputPaths"].as_object().unwrap().is_empty());
    }
}

#[tokio::test]
async fn w1_frame_failure_preserves_other_frames_and_context() {
    let mut data = section(2);
    data.snapshot.nodes.remove("1:2");
    data.snapshot.roots.retain(|id| id != "1:2");
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export { all_screens, .. } = &mut op {
        *all_screens = true;
    }
    let result = project(data, op).await.unwrap();
    assert_eq!(result["status"], "partial");
    assert!(result["frames"][0]["tsx"].is_string());
    assert_eq!(result["failures"][0]["nodeId"], "1:2");
    assert_eq!(result["frames"][1]["quality"]["projection"], "failed");
    assert_eq!(
        result["failures"][0]["details"]["snapshotRootIds"],
        json!(["1:1"])
    );
}

#[tokio::test]
async fn w1_all_screens_uses_explore_classification() {
    let mut data = section(3);
    data.metadata["sectionIndex"]["candidates"][1]["bounds"] =
        json!({"x":0,"y":0,"width":900,"height":100});
    data.metadata["sectionIndex"]["candidates"][2]["bounds"] =
        json!({"x":0,"y":0,"width":100,"height":100});
    let mut op = operation(&["tsx"]);
    if let PendingOperation::Export { all_screens, .. } = &mut op {
        *all_screens = true;
    }
    let result = project(data, op).await.unwrap();
    assert_eq!(result["frames"].as_array().unwrap().len(), 1);
    assert_eq!(result["frames"][0]["nodeId"], "1:1");
}

#[tokio::test]
async fn w1_large_section_has_actionable_batch_refusal() {
    let mut op = operation(&["tsx", "assetManifest"]);
    if let PendingOperation::Export { all_screens, .. } = &mut op {
        *all_screens = true;
    }
    let error = project(section(14), op).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupInvalidInput);
    assert!(error.details["recommendedFrameIds"].is_array());
}

#[tokio::test]
async fn w1_resource_delivery_moves_all_code_and_updates_note() {
    let store = ArtifactStore::default();
    let data = payload();
    let key = ArtifactRequestKey::from_collection(&CollectionRequest::new(
        data.target.clone(),
        CollectionScope::Node,
    ));
    let artifact = store.insert(key, data).await.unwrap();
    let mut result = json!({"tsx":"screen", "responsiveTsx":"responsive", "componentTsx":"component",
            "frames":[{"componentTsx":"frame-component"}],
            "deliverable":{"note":"Implement from this value."}});
    let outputs = projected_outputs_from_result(result.as_object().unwrap()).unwrap();
    let attachment = apply_delivery(
        &mut result,
        DeliveryMode::Resource,
        &store,
        &artifact,
        outputs,
        None,
    )
    .await
    .unwrap();
    commit_delivery(attachment);
    assert!(result.get("componentTsx").is_none());
    assert!(result.get("responsiveTsx").is_none());
    assert!(result["frames"][0].get("componentTsx").is_none());
    assert_eq!(result["resources"].as_array().unwrap().len(), 4);
    assert!(
        result["deliverable"]["note"]
            .as_str()
            .unwrap()
            .contains("resources")
    );
}

#[test]
fn w1_asset_paths_are_ascii_bounded_and_windows_safe() {
    assert_eq!(
        host_safe_asset_path("/images/Frame 269.png"),
        "/images/Frame-269-3e2af8571e3f.png"
    );
    for name in [
        "한글.svg".to_owned(),
        "CON.svg".to_owned(),
        format!("{}.png", "a".repeat(300)),
    ] {
        let path = host_safe_asset_path(&format!("/icons/{name}"));
        assert!(path.is_ascii());
        assert!(path.len() < 200);
        assert_ne!(path.to_uppercase(), "/ICONS/CON.SVG");
        assert_eq!(host_safe_asset_path(&path), path);
    }
    assert_ne!(
        host_safe_asset_path("/icons/한글.svg"),
        host_safe_asset_path("/icons/한국.svg")
    );
}

#[test]
fn w1_fixture_asset_paths_are_normalized_without_changing_other_code() {
    let code = include_str!("../../../../../fixtures/plugin-answers/notice/pure.tsx");
    let safe = with_host_safe_asset_paths(code);
    assert!(!safe.contains("/icons/Property 1="));
    assert_eq!(with_host_safe_asset_paths(&safe), safe);
}
#[tokio::test]
async fn w1_index_accepts_component_projection_selection() {
    let store = ArtifactStore::default();
    let data = section(2);
    let key = ArtifactRequestKey::from_collection(&CollectionRequest::new(
        data.target.clone(),
        CollectionScope::Node,
    ));
    let artifact = store.insert(key, data).await.unwrap();
    for output in ["componentTsx", "responsiveTsx"] {
        super::super::validation::validate_artifact_projection(
            &artifact,
            &[output.into()],
            "node",
            &[],
        )
        .unwrap();
    }
}
