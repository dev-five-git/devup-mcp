

/// A page that does not move forward is refused, not asked for forever.
#[test]
fn a_fast_theme_page_that_does_not_advance_is_refused() {
    let mut request = CollectionRequest::new(file_target(), CollectionScope::File);
    request.resource_scope = ResourceScope::File;
    request.variables_only = true;
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(first) = collector.advance().unwrap() else {
        panic!("fast theme call expected")
    };
    let error = collector
        .accept(
            &first.id,
            fast_theme_page_result(
                json!({"collections": [], "variables": [], "styles": []}),
                json!({"offset": 0, "nextOffset": 0, "complete": false, "totalItems": 4}),
            ),
        )
        .expect_err("a page cursor that stands still is an error");
    assert_eq!(error.code, ErrorCode::DevupFigmaHandoffInvalid);
}

fn fast_theme_page_result(resources: Value, page: Value) -> UpstreamResult {
    // The scan is paged like the resources: this page's share of the used
    // ids, or none.
    let used_variable_ids = resources
        .get("usedVariableIds")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let used_style_ids = resources
        .get("usedStyleIds")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let mut envelope = json!({
        "kind": "devupFastThemeEnvelope",
        "schemaVersion": 1,
        "source": {"fileKey": "FileKey123", "version": "v2"},
        "resources": {
            "collections": resources["collections"],
            "variables": resources["variables"],
            "styles": resources["styles"],
            "usedRemoteVariables": [],
            "usedVariableIds": used_variable_ids,
            "usedStyleIds": used_style_ids,
            "localComplete": true,
            "usedRemoteComplete": true,
            "unresolved": []
        },
        "page": page,
        "integrity": {
            "collectionCount": resources["collections"].as_array().map_or(0, Vec::len),
            "variableCount": resources["variables"].as_array().map_or(0, Vec::len),
            "styleCount": resources["styles"].as_array().map_or(0, Vec::len),
            "unresolvedCount": 0,
            "utf8Bytes": 0
        }
    });
    loop {
        let bytes = serde_json::to_vec(&envelope).unwrap();
        if envelope["integrity"]["utf8Bytes"] == bytes.len() as u64 {
            break;
        }
        envelope["integrity"]["utf8Bytes"] = Value::from(bytes.len());
    }
    UpstreamResult {
        raw: json!({"content": [{"type": "text", "text": envelope.to_string()}]}),
    }
}

#[test]
fn malformed_or_rejected_fast_theme_restarts_legacy_collection_atomically() {
    let mut request = CollectionRequest::new(file_target(), CollectionScope::File);
    request.resource_scope = ResourceScope::File;
    request.variables_only = true;
    let mut malformed = CollectorSession::new(request.clone());
    let CollectorStep::Call(fast) = malformed.advance().unwrap() else {
        panic!("fast theme call expected")
    };
    malformed
        .accept(
            &fast.id,
            UpstreamResult {
                raw: json!({"content": [{"type": "text", "text": "corrupt"}]}),
            },
        )
        .unwrap();
    let CollectorStep::Call(metadata) = malformed.advance().unwrap() else {
        panic!("legacy metadata must restart from zero")
    };
    assert_eq!(metadata.call.tool_name(), "get_metadata");
    assert_eq!(metadata.expected_node_id, None);

    let mut rejected = CollectorSession::new(request);
    let CollectorStep::Call(fast) = rejected.advance().unwrap() else {
        panic!("fast theme call expected")
    };
    assert!(
        rejected
            .reject(
                &fast.id,
                &DevupError::new(
                    ErrorCode::DevupFigmaDirectUnavailable,
                    "fast theme unavailable",
                    true,
                ),
            )
            .unwrap()
    );
    let CollectorStep::Call(metadata) = rejected.advance().unwrap() else {
        panic!("rejected fast theme must restart legacy metadata")
    };
    assert_eq!(metadata.call.tool_name(), "get_metadata");
}

#[test]
fn node_collection_advances_from_metadata_to_snapshot_to_complete() {
    let request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    let mut collector = CollectorSession::new(request);

    let CollectorStep::Call(metadata_call) = collector.advance().unwrap() else {
        panic!("metadata call expected")
    };
    assert_eq!(metadata_call.call.tool_name(), "get_metadata");
    collector
        .accept(&metadata_call.id, metadata("FRAME", &[], 1))
        .unwrap();

    let CollectorStep::Call(snapshot_call) = collector.advance().unwrap() else {
        panic!("snapshot call expected")
    };
    assert_eq!(snapshot_call.call.tool_name(), "use_figma");
    assert_eq!(snapshot_call.expected_node_id.as_deref(), Some("1:2"));
    collector
        .accept(&snapshot_call.id, snapshot("1:2"))
        .unwrap();

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("collection should be complete")
    };
    assert_eq!(parts.target.node_id.as_deref(), Some("1:2"));
    assert_eq!(parts.source_version.as_deref(), Some("v1"));
    assert_eq!(parts.snapshot_chunks.len(), 1);
}

#[test]
fn large_page_is_split_in_declared_child_order() {
    let request = CollectionRequest::new(target("1:2"), CollectionScope::Page);
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(call) = collector.advance().unwrap() else {
        panic!()
    };
    collector
        .accept(&call.id, metadata("PAGE", &["1:3", "1:4"], 500))
        .unwrap();

    let CollectorStep::Call(first) = collector.advance().unwrap() else {
        panic!()
    };
    let CollectorStep::Call(second) = collector.advance().unwrap() else {
        panic!()
    };
    assert_eq!(first.expected_node_id.as_deref(), Some("1:3"));
    assert_eq!(second.expected_node_id.as_deref(), Some("1:4"));

    collector.accept(&second.id, snapshot("1:4")).unwrap();
    collector.accept(&first.id, snapshot("1:3")).unwrap();
    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!()
    };
    let root_order = parts
        .snapshot_chunks
        .iter()
        .flat_map(|chunk| chunk.root_ids.iter().map(String::as_str))
        .collect::<Vec<_>>();
    assert_eq!(root_order, ["1:3", "1:4"]);
}

#[test]
fn rejects_snapshot_from_a_different_file_version() {
    let request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(metadata_call) = collector.advance().unwrap() else {
        panic!()
    };
    collector
        .accept(&metadata_call.id, metadata("FRAME", &[], 1))
        .unwrap();
    let CollectorStep::Call(snapshot_call) = collector.advance().unwrap() else {
        panic!()
    };
    let mut wrong_version = snapshot("1:2");
    wrong_version.raw["version"] = json!("v2");

    let error = collector
        .accept(&snapshot_call.id, wrong_version)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::DevupFigmaVersionChanged);
}

#[test]
fn exposes_at_most_four_in_flight_calls() {
    let request = CollectionRequest::new(target("1:2"), CollectionScope::Page);
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(metadata_call) = collector.advance().unwrap() else {
        panic!()
    };
    collector
        .accept(
            &metadata_call.id,
            metadata("PAGE", &["1:3", "1:4", "1:5", "1:6", "1:7"], 500),
        )
        .unwrap();

    let mut issued = Vec::new();
    for _ in 0..4 {
        let CollectorStep::Call(call) = collector.advance().unwrap() else {
            panic!("four calls should be available")
        };
        issued.push(call);
    }
    assert!(matches!(
        collector.advance().unwrap(),
        CollectorStep::AwaitingResults
    ));

    collector.accept(&issued[0].id, snapshot("1:3")).unwrap();
    let CollectorStep::Call(fifth) = collector.advance().unwrap() else {
        panic!("the fifth call should be released after one result")
    };
    assert_eq!(fifth.expected_node_id.as_deref(), Some("1:7"));
}

#[test]
fn rejects_unknown_or_replayed_call_ids() {
    let request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(call) = collector.advance().unwrap() else {
        panic!()
    };

    let unknown = collector
        .accept("unknown", metadata("FRAME", &[], 1))
        .unwrap_err();
    assert_eq!(unknown.code, ErrorCode::DevupFigmaHandoffInvalid);

    collector
        .accept(&call.id, metadata("FRAME", &[], 1))
        .unwrap();
    let replayed = collector
        .accept(&call.id, metadata("FRAME", &[], 1))
        .unwrap_err();
    assert_eq!(replayed.code, ErrorCode::DevupFigmaHandoffInvalid);
}

#[test]
fn variable_collection_uses_catalog_then_batched_resources() {
    let mut request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    request.resource_scope = ResourceScope::File;
    let mut collector = CollectorSession::new(request);

    let CollectorStep::Call(metadata_call) = collector.advance().unwrap() else {
        panic!()
    };
    collector
        .accept(&metadata_call.id, metadata("FRAME", &[], 1))
        .unwrap();
    let CollectorStep::Call(snapshot_call) = collector.advance().unwrap() else {
        panic!()
    };
    collector
        .accept(&snapshot_call.id, snapshot("1:2"))
        .unwrap();

    let CollectorStep::Call(catalog_call) = collector.advance().unwrap() else {
        panic!("variable/style catalog should follow the snapshot")
    };
    let catalog_code = catalog_call.call.arguments()["code"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(catalog_code.contains("getLocalVariableCollectionsAsync"));
    assert!(!catalog_code.contains("getLocalVariablesAsync"));
    let catalog = UpstreamResult {
        raw: json!({
            "content": [{
                "type": "text",
                "text": "{\"collections\":[{\"id\":\"c1\",\"name\":\"Synthetic\",\"defaultModeId\":\"m1\",\"modes\":[]}],\"variableIds\":[\"v1\"],\"styles\":[{\"id\":\"s1\",\"styleType\":\"TEXT\"}],\"localComplete\":true,\"usedRemoteComplete\":false}"
            }]
        }),
    };
    collector.accept(&catalog_call.id, catalog).unwrap();

    let CollectorStep::Call(batch_call) = collector.advance().unwrap() else {
        panic!("resource batch should follow the catalog")
    };
    let batch_code = batch_call.call.arguments()["code"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(batch_code.contains("getVariableByIdAsync"));
    assert!(batch_code.contains("getStyleByIdAsync"));
    let variable_batch = UpstreamResult {
        raw: json!({
            "content": [{
                "type": "text",
                "text": "{\"variables\":[{\"id\":\"v1\",\"name\":\"Synthetic Variable\"}],\"styles\":[]}"
            }]
        }),
    };
    collector.accept(&batch_call.id, variable_batch).unwrap();
    let CollectorStep::Call(style_call) = collector.advance().unwrap() else {
        panic!("style batch should be separate")
    };
    collector
        .accept(
            &style_call.id,
            UpstreamResult {
                raw: json!({
                    "variables": [],
                    "styles": [{"id": "s1", "name": "Synthetic Style", "styleType": "TEXT", "value": {}}]
                }),
            },
        )
        .unwrap();

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("collection should finish after variables")
    };
    let merged = &parts.variables.as_ref().unwrap().raw;
    assert_eq!(merged["collections"][0]["id"], "c1");
    assert_eq!(merged["variables"][0]["id"], "v1");
    assert_eq!(merged["styles"][0]["id"], "s1");
    assert_eq!(parts.styles.as_ref().unwrap().raw, *merged);
}

#[test]
fn a_file_theme_reads_the_library_resources_its_nodes_use_and_its_variables_alias() {
    let mut request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    request.resource_scope = ResourceScope::File;
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(metadata_call) = collector.advance().unwrap() else {
        panic!()
    };
    collector
        .accept(&metadata_call.id, metadata("FRAME", &[], 1))
        .unwrap();
    let CollectorStep::Call(snapshot_call) = collector.advance().unwrap() else {
        panic!()
    };
    // The node pads with a local variable and fills with a library one.
    collector
        .accept(
            &snapshot_call.id,
            UpstreamResult {
                raw: json!({
                    "fileKey": "FileKey123", "version": "v1", "rootIds": ["1:2"],
                    "nodes": [{"id": "1:2", "type": "FRAME", "fields": {
                        "name": "Frame", "childrenIds": [],
                        "boundVariables": {"paddingLeft": {"type": "VARIABLE_ALIAS", "id": "VariableID:1:1"}},
                        "fills": [{"type": "SOLID", "boundVariables": {"color": {"type": "VARIABLE_ALIAS", "id": "VariableID:lib/2:2"}}, "extra": {}, "fieldErrors": {}}]
                    }}]
                }),
            },
        )
        .unwrap();
    let CollectorStep::Call(catalog_call) = collector.advance().unwrap() else {
        panic!()
    };
    collector
        .accept(
            &catalog_call.id,
            UpstreamResult {
                raw: json!({
                    "collections": [{"id": "c1", "name": "Local", "defaultModeId": "m", "modes": [{"modeId": "m", "name": "Mobile"}]}],
                    "variableIds": ["VariableID:1:1"], "styles": [],
                    "localComplete": true, "usedRemoteComplete": false
                }),
            },
        )
        .unwrap();
    let CollectorStep::Call(local_batch) = collector.advance().unwrap() else {
        panic!()
    };
    // The local variable aliases a library primitive no node binds.
    collector
        .accept(
            &local_batch.id,
            UpstreamResult {
                raw: json!({"variables": [{"id": "VariableID:1:1", "name": "containerX", "variableCollectionId": "c1",
                    "valuesByMode": {"m": {"type": "VARIABLE_ALIAS", "id": "VariableID:lib/3:3"}}}], "styles": []}),
            },
        )
        .unwrap();
    let CollectorStep::Call(library_call) = collector.advance().unwrap() else {
        panic!("the library resources should be read after the catalog")
    };
    let code = library_call.call.arguments()["code"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        code.contains("VariableID:lib/2:2"),
        "the bound library variable is read"
    );
    assert!(
        code.contains("VariableID:lib/3:3"),
        "the aliased library variable is read"
    );
    assert!(
        !code.contains("\"VariableID:1:1\""),
        "the local variable is not read again"
    );
    collector
        .accept(
            &library_call.id,
            UpstreamResult {
                raw: json!({
                    "collections": [{"id": "lib", "name": "Library", "defaultModeId": "x", "modes": [{"modeId": "x", "name": "Mode 1"}]}],
                    "variables": [
                        {"id": "VariableID:lib/2:2", "name": "gray", "remote": true, "variableCollectionId": "lib", "valuesByMode": {"x": {"r": 0, "g": 0, "b": 0, "a": 1}}},
                        {"id": "VariableID:lib/3:3", "name": "spacing20", "remote": true, "variableCollectionId": "lib", "valuesByMode": {"x": {"type": "VARIABLE_ALIAS", "id": "VariableID:lib/4:4"}}},
                        // What the read followed: the primitive the spacing aliases.
                        {"id": "VariableID:lib/4:4", "name": "base20", "remote": true, "variableCollectionId": "lib", "valuesByMode": {"x": 20}}
                    ],
                    "styles": [], "unresolved": []
                }),
            },
        )
        .unwrap();
    let step = collector.advance().unwrap();
    let CollectorStep::Complete(parts) = step else {
        panic!("collection should finish after the library resources: {step:?}")
    };
    let raw = &parts.variables.as_ref().unwrap().raw;
    let ids = |key: &str| {
        raw[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value["id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids("variables"),
        [
            "VariableID:1:1",
            "VariableID:lib/2:2",
            "VariableID:lib/3:3",
            "VariableID:lib/4:4"
        ]
    );
    assert_eq!(
        ids("usedRemoteVariables"),
        [
            "VariableID:lib/2:2",
            "VariableID:lib/3:3",
            "VariableID:lib/4:4"
        ]
    );
    assert_eq!(ids("collections"), ["c1", "lib"]);
    assert_eq!(raw["localComplete"], true);
    assert_eq!(raw["usedRemoteComplete"], true);
}

#[test]
fn accepts_the_official_xml_metadata_envelope_without_inventing_values() {
    let request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(metadata_call) = collector.advance().unwrap() else {
        panic!()
    };

    collector
        .accept(&metadata_call.id, official_xml_metadata())
        .unwrap();
    let CollectorStep::Call(snapshot_call) = collector.advance().unwrap() else {
        panic!()
    };
    assert_eq!(snapshot_call.expected_file_key, "FileKey123");
    assert_eq!(snapshot_call.expected_node_id.as_deref(), Some("1:2"));
}

#[test]
fn variable_batches_merge_in_catalog_order_when_results_arrive_out_of_order() {
    let mut request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    request.resource_scope = ResourceScope::File;
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(metadata_call) = collector.advance().unwrap() else {
        panic!()
    };
    collector
        .accept(&metadata_call.id, metadata("FRAME", &[], 1))
        .unwrap();
    let CollectorStep::Call(snapshot_call) = collector.advance().unwrap() else {
        panic!()
    };
    collector
        .accept(&snapshot_call.id, snapshot("1:2"))
        .unwrap();
    let CollectorStep::Call(catalog_call) = collector.advance().unwrap() else {
        panic!()
    };
    let variable_ids = (0..9)
        .map(|index| format!("v{index:02}"))
        .collect::<Vec<_>>();
    let expected_ids = variable_ids.clone();
    collector
        .accept(
            &catalog_call.id,
            UpstreamResult {
                raw: json!({
                    "collections": [],
                    "variableIds": variable_ids,
                    "styles": [],
                    "localComplete": true,
                    "usedRemoteComplete": false
                }),
            },
        )
        .unwrap();
    let CollectorStep::Call(first) = collector.advance().unwrap() else {
        panic!()
    };
    let CollectorStep::Call(second) = collector.advance().unwrap() else {
        panic!()
    };
    let first_values = (0..8)
        .map(|index| json!({"id": format!("v{index:02}")}))
        .collect::<Vec<_>>();
    collector
        .accept(
            &second.id,
            UpstreamResult {
                raw: json!({"variables": [{"id": "v08"}], "styles": []}),
            },
        )
        .unwrap();
    collector
        .accept(
            &first.id,
            UpstreamResult {
                raw: json!({"variables": first_values, "styles": []}),
            },
        )
        .unwrap();
    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!()
    };
    let variables = parts.variables.unwrap();
    let ids = variables.raw["variables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|variable| variable["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(ids, expected_ids);
}

#[test]
fn style_consumers_are_planned_as_compact_bounded_fragments() {
    let mut request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    request.resource_scope = ResourceScope::File;
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(metadata_call) = collector.advance().unwrap() else {
        panic!()
    };
    collector
        .accept(&metadata_call.id, metadata("FRAME", &[], 1))
        .unwrap();
    let CollectorStep::Call(snapshot_call) = collector.advance().unwrap() else {
        panic!()
    };
    collector
        .accept(&snapshot_call.id, snapshot("1:2"))
        .unwrap();
    let CollectorStep::Call(catalog_call) = collector.advance().unwrap() else {
        panic!()
    };
    collector
        .accept(
            &catalog_call.id,
            UpstreamResult {
                raw: json!({
                    "collections": [],
                    "variableIds": [],
                    "styles": [
                        {"id": "s1", "styleType": "TEXT"},
                        {"id": "s2", "styleType": "PAINT"}
                    ],
                    "localComplete": true,
                    "usedRemoteComplete": false
                }),
            },
        )
        .unwrap();

    let CollectorStep::Call(base_styles) = collector.advance().unwrap() else {
        panic!("style definitions should be batched")
    };
    let base_arguments = base_styles.call.arguments();
    let base_code = base_arguments["code"].as_str().unwrap();
    assert!(base_code.contains("s1"));
    assert!(base_code.contains("s2"));
    collector
        .accept(
            &base_styles.id,
            UpstreamResult {
                raw: json!({
                    "variables": [],
                    "styles": [
                        {"id": "s1", "name": "Text", "styleType": "TEXT", "value": {}, "$consumerCount": 321},
                        {"id": "s2", "name": "Paint", "styleType": "PAINT", "value": [], "$consumerCount": 0}
                    ]
                }),
            },
        )
        .unwrap();

    let CollectorStep::Call(first) = collector.advance().unwrap() else {
        panic!("first consumer fragment should be scheduled")
    };
    let CollectorStep::Call(second) = collector.advance().unwrap() else {
        panic!("second consumer fragment should be scheduled")
    };
    let first_arguments = first.call.arguments();
    let second_arguments = second.call.arguments();
    let first_code = first_arguments["code"].as_str().unwrap();
    let second_code = second_arguments["code"].as_str().unwrap();
    assert!(first_code.contains("s1"));
    assert!(first_code.contains("\"consumerStart\":0"));
    assert!(first_code.contains("\"consumerEnd\":320"));
    assert!(second_code.contains("s1"));
    assert!(second_code.contains("\"consumerStart\":320"));
    assert!(second_code.contains("\"consumerEnd\":321"));
}

#[test]
fn used_scope_resolves_only_snapshot_references_and_keeps_partial_results() {
    let mut request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    request.resource_scope = ResourceScope::Used;
    let mut collector = CollectorSession::new(request);

    let CollectorStep::Call(fast_call) = collector.advance().unwrap() else {
        panic!("fast call expected")
    };
    collector
        .accept(
            &fast_call.id,
            UpstreamResult {
                raw: json!({"content": [{"type": "text", "text": "fallback"}]}),
            },
        )
        .unwrap();
    let CollectorStep::Call(metadata_call) = collector.advance().unwrap() else {
        panic!("metadata call expected")
    };
    collector
        .accept(&metadata_call.id, metadata("FRAME", &[], 1))
        .unwrap();
    let CollectorStep::Call(snapshot_call) = collector.advance().unwrap() else {
        panic!("snapshot call expected")
    };
    let mut used_snapshot = snapshot("1:2");
    used_snapshot.raw["nodes"][0]["fields"] = json!({
        "name": "Synthetic",
        "boundVariables": {
            "fills": [{"type": "VARIABLE_ALIAS", "id": "VariableID:1:2"}]
        },
        "textStyleId": "S:text"
    });
    collector.accept(&snapshot_call.id, used_snapshot).unwrap();

    let CollectorStep::Call(resource_call) = collector.advance().unwrap() else {
        panic!("combined used resource batch should follow the snapshot")
    };
    let resource_code = resource_call.call.arguments()["code"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(resource_code.contains("getVariableByIdAsync"));
    assert!(resource_code.contains("VariableID:1:2"));
    assert!(resource_code.contains("S:text"));
    assert!(!resource_code.contains("getStyleConsumersAsync"));

    collector
        .accept(
            &resource_call.id,
            UpstreamResult {
                raw: json!({
                    "collections": [{
                        "id": "collection:1",
                        "name": "Foundation",
                        "defaultModeId": "mode:1",
                        "modes": [{"modeId": "mode:1", "name": "Default"}]
                    }],
                    "variables": [{
                        "id": "VariableID:1:2",
                        "name": "primary",
                        "remote": true,
                        "variableCollectionId": "collection:1",
                        "resolvedType": "COLOR",
                        "valuesByMode": {"mode:1": {"r": 1, "g": 0, "b": 0, "a": 1}}
                    }],
                    "styles": [],
                    "unresolved": [{"id": "S:text", "kind": "style", "reason": "notFoundOrUnavailable"}]
                }),
            },
        )
        .unwrap();

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("used resource collection should complete")
    };
    let resources = &parts.variables.as_ref().unwrap().raw;
    assert_eq!(resources["collections"][0]["id"], "collection:1");
    assert_eq!(resources["variables"][0]["name"], "primary");
    assert_eq!(resources["styles"], json!([]));
    assert_eq!(resources["usedRemoteComplete"], false);
    assert_eq!(resources["unresolved"][0]["id"], "S:text");
    assert_eq!(resources["usedVariableIds"], json!(["VariableID:1:2"]));
    assert_eq!(resources["usedStyleIds"], json!(["S:text"]));
    let diagnostic = parts.snapshot_chunks[0]
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "DEVUP_RESOURCE_UNRESOLVED")
        .expect("unresolved resource diagnostic");
    assert_eq!(diagnostic.node_id.as_deref(), Some("1:2"));
    assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::Warning));
    assert_eq!(diagnostic.property.as_deref(), Some("textStyleId"));
    assert_eq!(diagnostic.resource_kind.as_deref(), Some("style"));
    assert_eq!(diagnostic.resource_id.as_deref(), Some("S:text"));
    assert_eq!(diagnostic.fallback.as_deref(), Some("raw-value"));
    assert_eq!(diagnostic.recoverable, Some(true));
}

#[test]
fn explore_collection_starts_with_one_bounded_projection_and_no_resources() {
    let mut request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    request.explore = Some(ExploreReadOptions::default());
    request.resource_scope = ResourceScope::None;
    let mut collector = CollectorSession::new(request);

    let CollectorStep::Call(explore_call) = collector.advance().unwrap() else {
        panic!("explore projection call expected")
    };
    assert_eq!(explore_call.call.tool_name(), "use_figma");
    assert_eq!(explore_call.expected_node_id.as_deref(), Some("1:2"));
    let code = explore_call.call.arguments()["code"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(code.contains("projectionTruncated"));
    assert!(!code.contains("getLocalVariableCollectionsAsync"));
    assert!(!code.contains("getVariableByIdAsync"));

    collector
        .accept(
            &explore_call.id,
            UpstreamResult {
                raw: json!({
                    "fileKey": "FileKey123",
                    "version": null,
                    "rootIds": ["0:1"],
                    "nodes": [
                        {
                            "id": "0:1", "type": "PAGE",
                            "fields": {"name": "Page", "x": 0, "y": 0, "width": 1000, "height": 1000, "projectionTruncated": false},
                            "extra": {}, "fieldErrors": {}
                        },
                        {
                            "id": "1:2", "type": "FRAME",
                            "fields": {"name": "Heading", "parentId": "0:1", "x": 0, "y": 0, "width": 1000, "height": 80, "childCount": 1},
                            "extra": {}, "fieldErrors": {}
                        }
                    ],
                    "diagnostics": []
                }),
            },
        )
        .unwrap();

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("explore collection should complete after one projection")
    };
    assert_eq!(parts.snapshot_chunks.len(), 1);
    assert_eq!(parts.snapshot_chunks[0].nodes.len(), 2);
    assert!(parts.variables.is_none());
}

#[test]
fn node_snapshot_follows_the_compiled_cursor_until_complete() {
    let request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(metadata_call) = collector.advance().unwrap() else {
        panic!("metadata call expected")
    };
    collector
        .accept(&metadata_call.id, metadata("FRAME", &[], 2))
        .unwrap();

    let CollectorStep::Call(first_call) = collector.advance().unwrap() else {
        panic!("first snapshot chunk expected")
    };
    assert!(
        first_call.call.arguments()["code"]
            .as_str()
            .unwrap()
            .contains("\"offset\":0")
    );
    collector
        .accept(
            &first_call.id,
            UpstreamResult {
                raw: json!({
                    "fileKey": "FileKey123", "version": "v1", "rootIds": ["1:2"],
                    "nodes": [
                        {"id": "1:2", "type": "FRAME", "fields": {"name": "Root", "childrenIds": ["1:3"]}, "extra": {}, "fieldErrors": {}},
                        {"id": "__DEVUP_SNAPSHOT_CURSOR__", "type": "DEVUP_INTERNAL", "fields": {"offset":0,"nextOffset": 1, "complete": false, "totalNodes": 2}, "extra": {}, "fieldErrors": {}}
                    ], "diagnostics": []
                }),
            },
        )
        .unwrap();

    let CollectorStep::Call(second_call) = collector.advance().unwrap() else {
        panic!("second snapshot chunk expected")
    };
    assert!(
        second_call.call.arguments()["code"]
            .as_str()
            .unwrap()
            .contains("\"offset\":1")
    );
    collector
        .accept(
            &second_call.id,
            UpstreamResult {
                raw: json!({
                    "fileKey": "FileKey123", "version": "v1", "rootIds": ["1:2"],
                    "nodes": [
                        {"id": "1:3", "type": "TEXT", "fields": {"name": "Child", "characters": "Done", "childrenIds": []}, "extra": {}, "fieldErrors": {}},
                        {"id": "__DEVUP_SNAPSHOT_CURSOR__", "type": "DEVUP_INTERNAL", "fields": {"offset":0,"nextOffset": 2, "complete": true, "totalNodes": 2}, "extra": {}, "fieldErrors": {}}
                    ], "diagnostics": []
                }),
            },
        )
        .unwrap();

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("snapshot pagination should complete")
    };
    assert_eq!(parts.snapshot_chunks.len(), 2);
    assert!(
        parts
            .snapshot_chunks
            .iter()
            .flat_map(|chunk| &chunk.nodes)
            .all(|node| node.id != "__DEVUP_SNAPSHOT_CURSOR__")
    );
}

#[test]
fn section_collection_indexes_before_planning_selected_roots() {
    let mut request = CollectionRequest::new(target("10:1"), CollectionScope::Node);
    request.resource_scope = ResourceScope::Used;
    request.section = Some(SectionReadOptions {
        frame_ids: vec!["10:2".to_owned(), "10:3".to_owned()],
        all_screens: false,
    });
    let mut collector = CollectorSession::new(request);

    let CollectorStep::Call(index_call) = collector.advance().unwrap() else {
        panic!("compact section index expected")
    };
    assert!(
        index_call.call.arguments()["code"]
            .as_str()
            .unwrap()
            .contains("subtreeNodeCount")
    );
    collector
        .accept(&index_call.id, compact_section_index())
        .unwrap();

    let CollectorStep::Call(first_root_call) = collector.advance().unwrap() else {
        panic!("first selected root call expected")
    };
    let CollectorStep::Call(second_root_call) = collector.advance().unwrap() else {
        panic!("second selected root call expected")
    };
    assert_eq!(multi_root_ids(&first_root_call.call), ["10:3"]);
    assert_eq!(multi_root_ids(&second_root_call.call), ["10:2"]);
    assert_eq!(first_root_call.expected_node_id.as_deref(), Some("10:1"));
}

#[test]
fn rejected_exact_section_probe_pivots_to_the_compact_index() {
    let mut request = CollectionRequest::new(target("10:1"), CollectionScope::Node);
    request.resource_scope = ResourceScope::Used;
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(fast_call) = collector.advance().unwrap() else {
        panic!("fast section probe expected")
    };

    let recovered = collector
        .reject(
            &fast_call.id,
            &DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "Error: DEVUP_TARGET_IS_SECTION",
                false,
            ),
        )
        .unwrap();

    assert!(recovered);
    let CollectorStep::Call(index_call) = collector.advance().unwrap() else {
        panic!("compact section index expected")
    };
    assert!(
        index_call.call.arguments()["code"]
            .as_str()
            .unwrap()
            .contains("subtreeNodeCount")
    );
}

#[test]
fn failed_fast_and_legacy_section_root_is_reported_without_losing_siblings() {
    let mut request = CollectionRequest::new(target("10:1"), CollectionScope::Node);
    request.resource_scope = ResourceScope::Used;
    request.section = Some(SectionReadOptions {
        frame_ids: vec!["root-0".to_owned(), "root-1".to_owned()],
        all_screens: false,
    });
    request.cached_section_index = Some(section_index_with_node_counts(&[3_000, 3_000]));
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(first) = collector.advance().unwrap() else {
        panic!()
    };
    let CollectorStep::Call(second) = collector.advance().unwrap() else {
        panic!()
    };
    collector
        .accept(
            &second.id,
            fast_multi_envelope_result(&["root-1"], &["variable-success"]),
        )
        .unwrap();
    assert!(
        collector
            .reject(
                &first.id,
                &DevupError::new(ErrorCode::DevupFigmaDirectUnavailable, "fast failed", true,)
            )
            .unwrap()
    );
    let CollectorStep::Call(legacy) = collector.advance().unwrap() else {
        panic!("legacy retry expected")
    };
    assert!(
        collector
            .reject(
                &legacy.id,
                &DevupError::new(
                    ErrorCode::DevupFigmaDirectUnavailable,
                    "legacy failed",
                    true,
                )
            )
            .unwrap()
    );

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("successful sibling should complete")
    };
    assert_eq!(
        merge_chunks(parts.snapshot_chunks).unwrap().roots,
        ["root-1"]
    );
    assert_eq!(parts.failures.len(), 1);
    assert_eq!(parts.failures[0].node_id, "root-0");
    assert_eq!(
        parts.failures[0].error_code,
        ErrorCode::DevupFigmaDirectUnavailable
    );
}