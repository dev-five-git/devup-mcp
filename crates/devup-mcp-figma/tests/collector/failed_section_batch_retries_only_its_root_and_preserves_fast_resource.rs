

#[test]
fn failed_section_batch_retries_only_its_root_and_preserves_fast_resources() {
    let mut request = CollectionRequest::new(target("10:1"), CollectionScope::Node);
    request.resource_scope = ResourceScope::Used;
    request.section = Some(SectionReadOptions {
        frame_ids: vec![
            "root-0".to_owned(),
            "root-1".to_owned(),
            "root-2".to_owned(),
        ],
        all_screens: false,
    });
    request.cached_section_index = Some(three_batch_section_index());
    let mut collector = CollectorSession::new(request);

    let CollectorStep::Call(first_fast) = collector.advance().unwrap() else {
        panic!("first fast batch expected")
    };
    let CollectorStep::Call(second_fast) = collector.advance().unwrap() else {
        panic!("second fast batch expected")
    };
    assert_eq!(multi_root_ids(&first_fast.call), ["root-0"]);
    assert_eq!(multi_root_ids(&second_fast.call), ["root-1"]);
    collector
        .accept(
            &second_fast.id,
            fast_multi_envelope_result(&["root-1"], &["variable-success-1"]),
        )
        .unwrap();
    assert!(
        collector
            .reject(
                &first_fast.id,
                &DevupError::new(
                    ErrorCode::DevupFigmaDirectUnavailable,
                    "synthetic fast batch failure",
                    true,
                ),
            )
            .unwrap()
    );

    let CollectorStep::Call(third_fast) = collector.advance().unwrap() else {
        panic!("the untouched third fast batch must remain queued")
    };
    assert_eq!(multi_root_ids(&third_fast.call), ["root-2"]);
    let CollectorStep::Call(failed_root_retry) = collector.advance().unwrap() else {
        panic!("only the failed root must be retried through the legacy cursor")
    };
    assert_eq!(legacy_root_id(&failed_root_retry.call), "root-0");
    collector
        .accept(
            &third_fast.id,
            fast_multi_envelope_result(&["root-2"], &["variable-success-2"]),
        )
        .unwrap();
    let mut failed_snapshot = snapshot("root-0");
    failed_snapshot.raw["nodes"][0]["fields"] = json!({
        "name": "Failed fast root",
        "boundVariables": {
            "fills": [{"type": "VARIABLE_ALIAS", "id": "variable-fallback"}]
        }
    });
    collector
        .accept(&failed_root_retry.id, failed_snapshot)
        .unwrap();

    let CollectorStep::Call(missing_resources) = collector.advance().unwrap() else {
        panic!("only resources missing from the failed fast batch should be acquired")
    };
    let resource_code = missing_resources.call.arguments()["code"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(resource_code.contains("variable-fallback"));
    assert!(!resource_code.contains("variable-success-1"));
    assert!(!resource_code.contains("variable-success-2"));
    collector
        .accept(
            &missing_resources.id,
            UpstreamResult {
                raw: json!({
                    "collections": [],
                    "variables": [{"id": "variable-fallback", "name": "Fallback"}],
                    "styles": []
                }),
            },
        )
        .unwrap();

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("partial fast fallback should complete")
    };
    assert_eq!(parts.stats.figma_tool_calls, 5);
    assert_eq!(parts.stats.transport, "hybrid-multi-root-cursor");
    assert_eq!(
        merge_chunks(parts.snapshot_chunks.clone()).unwrap().roots,
        ["root-0", "root-1", "root-2"]
    );
    let resources = parts.variables.unwrap();
    let variable_ids = resources.raw["variables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|variable| variable["id"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        variable_ids,
        std::collections::BTreeSet::from([
            "variable-fallback",
            "variable-success-1",
            "variable-success-2"
        ])
    );
}

#[test]
fn oversized_section_root_uses_legacy_once_and_preserves_fast_resources() {
    let mut request = CollectionRequest::new(target("10:1"), CollectionScope::Node);
    request.resource_scope = ResourceScope::Used;
    request.section = Some(SectionReadOptions {
        frame_ids: vec!["root-0".to_owned(), "root-1".to_owned()],
        all_screens: false,
    });
    request.cached_section_index = Some(section_index_with_node_counts(&[1_000, 5_000]));
    let mut collector = CollectorSession::new(request);

    let CollectorStep::Call(fast) = collector.advance().unwrap() else {
        panic!("bounded root should use the fast batch")
    };
    let CollectorStep::Call(oversized) = collector.advance().unwrap() else {
        panic!("oversized root should be scheduled once through the legacy cursor")
    };
    assert_eq!(multi_root_ids(&fast.call), ["root-0"]);
    assert_eq!(legacy_root_id(&oversized.call), "root-1");
    collector
        .accept(
            &fast.id,
            fast_multi_envelope_result(&["root-0"], &["variable-fast"]),
        )
        .unwrap();
    let mut oversized_snapshot = snapshot("root-1");
    oversized_snapshot.raw["nodes"][0]["fields"] = json!({
        "name": "Oversized root",
        "boundVariables": {
            "fills": [{"type": "VARIABLE_ALIAS", "id": "variable-oversized"}]
        }
    });
    collector.accept(&oversized.id, oversized_snapshot).unwrap();

    let CollectorStep::Call(resources) = collector.advance().unwrap() else {
        panic!("the oversized root's missing resources should be collected")
    };
    let resource_arguments = resources.call.arguments();
    let resource_code = resource_arguments["code"].as_str().unwrap();
    assert!(resource_code.contains("variable-oversized"));
    assert!(!resource_code.contains("variable-fast"));
    collector
        .accept(
            &resources.id,
            UpstreamResult {
                raw: json!({
                    "collections": [],
                    "variables": [{"id": "variable-oversized", "name": "Oversized"}],
                    "styles": []
                }),
            },
        )
        .unwrap();

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("hybrid oversized collection should complete")
    };
    assert_eq!(parts.stats.figma_tool_calls, 3);
    assert_eq!(parts.stats.transport, "hybrid-multi-root-cursor");
    assert_eq!(
        merge_chunks(parts.snapshot_chunks.clone()).unwrap().roots,
        ["root-0", "root-1"]
    );
    let variables = parts.variables.unwrap();
    assert_eq!(variables.raw["variables"].as_array().unwrap().len(), 2);
}

#[test]
fn section_collection_without_selection_completes_after_the_compact_index() {
    let mut request = CollectionRequest::new(target("10:1"), CollectionScope::Node);
    request.resource_scope = ResourceScope::Used;
    request.section = Some(SectionReadOptions {
        frame_ids: Vec::new(),
        all_screens: false,
    });
    let mut collector = CollectorSession::new(request);

    let CollectorStep::Call(index_call) = collector.advance().unwrap() else {
        panic!("compact section index expected")
    };
    collector
        .accept(&index_call.id, compact_section_index())
        .unwrap();
    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("selection discovery must not collect a full subtree")
    };

    assert_eq!(parts.stats.figma_tool_calls, 1);
    assert_eq!(parts.snapshot_chunks.len(), 1);
    assert_eq!(parts.snapshot_chunks[0].nodes.len(), 3);
    assert_eq!(
        parts.metadata["sectionIndex"]["candidates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

fn compact_section_index() -> UpstreamResult {
    UpstreamResult {
        raw: json!({
            "fileKey": "FileKey123", "version": null, "rootIds": ["10:1"],
            "nodes": [
                {"id": "10:1", "type": "SECTION", "fields": {
                    "name": "Proofread", "parentId": "0:1", "childrenIds": ["10:2", "10:3"],
                    "visible": true, "projectionTruncated": false,
                    "absoluteBoundingBox": {"x": 0, "y": 0, "width": 1200, "height": 1000}
                }, "extra": {}, "fieldErrors": {}},
                {"id": "10:2", "type": "FRAME", "fields": {
                    "name": "Second", "parentId": "10:1", "childrenIds": [], "visible": true,
                    "directChildCount": 5, "subtreeNodeCount": 20, "estimatedSerializedBytes": 10000,
                    "absoluteBoundingBox": {"x": 500, "y": 120, "width": 360, "height": 740}
                }, "extra": {}, "fieldErrors": {}},
                {"id": "10:3", "type": "FRAME", "fields": {
                    "name": "First", "parentId": "10:1", "childrenIds": [], "visible": true,
                    "directChildCount": 5, "subtreeNodeCount": 20, "estimatedSerializedBytes": 10000,
                    "absoluteBoundingBox": {"x": 100, "y": 120, "width": 360, "height": 740}
                }, "extra": {}, "fieldErrors": {}}
            ],
            "diagnostics": []
        }),
    }
}

fn three_batch_section_index() -> SectionIndex {
    section_index_with_node_counts(&[3_000, 3_000, 3_000])
}

fn section_index_with_node_counts(node_counts: &[usize]) -> SectionIndex {
    let section_bounds = ExploreBounds {
        x: 0.0,
        y: 0.0,
        width: 1000.0,
        height: 1000.0,
    };
    SectionIndex {
        node_screen_ids: std::collections::BTreeMap::new(),
        file_key: "FileKey123".to_owned(),
        source_version: Some("v1".to_owned()),
        section: SectionSummary {
            node_id: "10:1".to_owned(),
            name: "Section".to_owned(),
            bounds: section_bounds,
        },
        candidates: node_counts
            .iter()
            .enumerate()
            .map(|(index, node_count)| SectionCandidate {
                node_id: format!("root-{index}"),
                name: format!("Root {index}"),
                node_type: "FRAME".to_owned(),
                text_preview: String::new(),
                text_preview_state: None,
                navigation_content_width: None,
                visible: true,
                bounds: ExploreBounds {
                    y: index as f64 * 200.0,
                    ..section_bounds
                },
                parent_id: Some("10:1".to_owned()),
                breadcrumb: vec!["Section".to_owned(), format!("Root {index}")],
                direct_child_count: 0,
                subtree_node_count: *node_count,
                estimated_serialized_bytes: 1_000,
                selection_reasons: vec!["screen-like".to_owned()],
                canonical_url: format!(
                    "https://www.figma.com/design/FileKey123/devup?node-id=root-{index}"
                ),
            })
            .collect(),
        truncated: false,
    }
}

fn multi_root_ids(call: &ReadToolCall) -> Vec<&str> {
    let ReadToolCall::Snapshot {
        script: BuiltinScript::MultiRootSnapshotEnvelope,
        root_ids: Some(root_ids),
        ..
    } = call
    else {
        panic!("multi-root fast snapshot call expected")
    };
    root_ids.iter().map(String::as_str).collect()
}

fn boolean_logo_page(root: &str, offset: usize) -> UpstreamResult {
    let variable = format!("{root}-variable-{offset}");
    let base = fast_multi_envelope_result(&[root], &[&variable]);
    let mut envelope: Value =
        serde_json::from_str(base.raw["content"][0]["text"].as_str().unwrap()).unwrap();
    let union = format!("{root}-union");
    let operands = (0..6)
        .map(|i| format!("{root}-vector-{i}"))
        .collect::<Vec<_>>();
    let mut first = envelope["snapshot"]["nodes"][0].clone();
    let mut nodes = if offset == 0 {
        first["fields"]["childrenIds"] = json!([union]);
        vec![
            first,
            json!({"id": union, "type": "BOOLEAN_OPERATION", "fields": {"parentId": root, "childrenIds": operands}}),
        ]
    } else {
        first["id"] = json!(operands[0]);
        first["type"] = json!("VECTOR");
        first["fields"]["parentId"] = json!(union);
        let mut nodes = vec![first];
        nodes.extend(
            operands
                .iter()
                .skip(1)
                .map(|id| json!({"id": id, "type": "VECTOR", "fields": {"parentId": union}})),
        );
        nodes
    };
    nodes.push(json!({"id": "__DEVUP_SNAPSHOT_CURSOR__", "type": "DEVUP_INTERNAL", "fields": {
        "offset": offset, "nextOffset": if offset == 0 {2} else {8}, "totalNodes": 8, "complete": offset != 0
    }}));
    envelope["integrity"]["nodeCount"] = json!(nodes.len());
    envelope["snapshot"]["nodes"] = json!(nodes);
    UpstreamResult {
        raw: json!({"content": [{"type": "text", "text": envelope.to_string()}]}),
    }
}

#[test]
fn section_boolean_operands_are_collected_from_each_roots_continuation() {
    let mut request = CollectionRequest::new(target("10:1"), CollectionScope::Node);
    request.resource_scope = ResourceScope::Used;
    request.section = Some(SectionReadOptions {
        frame_ids: vec!["root-0".into(), "root-1".into()],
        all_screens: false,
    });
    request.cached_section_index = Some(section_index_with_node_counts(&[8, 8]));
    let mut collector = CollectorSession::new(request);
    let mut first = Vec::new();
    for _ in 0..2 {
        let CollectorStep::Call(call) = collector.advance().unwrap() else {
            panic!("first page expected")
        };
        first.push(call);
    }
    // Complete siblings out of order, as the server does with concurrent reads.
    for call in first.iter().rev() {
        let root = multi_root_ids(&call.call)[0];
        collector
            .accept(&call.id, boolean_logo_page(root, 0))
            .unwrap();
    }
    for _ in 0..2 {
        let CollectorStep::Call(call) = collector.advance().unwrap() else {
            panic!("six boolean operands still need a continuation page")
        };
        let root = multi_root_ids(&call.call)[0];
        assert_eq!(call.expected_node_id.as_deref(), Some("10:1"));
        assert!(
            call.call.arguments()["code"]
                .as_str()
                .unwrap()
                .contains("\"offset\":2")
        );
        collector
            .accept(&call.id, boolean_logo_page(root, 2))
            .unwrap();
    }
    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("all pages should complete")
    };
    let snapshot = merge_chunks(parts.snapshot_chunks).unwrap();
    assert_eq!(snapshot.nodes.len(), 16);
    assert!(snapshot.audit().missing_children.is_empty());
    assert!(!snapshot.nodes.contains_key("__DEVUP_SNAPSHOT_CURSOR__"));
    assert_eq!(
        parts.variables.unwrap().raw["variables"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
}

#[test]
fn section_pages_reject_mismatched_or_nonadvancing_cursors() {
    for (field, value) in [
        ("offset", json!(1)),
        ("nextOffset", json!(0)),
        ("nextOffset", json!(3)),
        ("totalNodes", json!(1)),
        // nextOffset == totalNodes requires complete: true.
        ("totalNodes", json!(2)),
    ] {
        let mut request = CollectionRequest::new(target("10:1"), CollectionScope::Node);
        request.resource_scope = ResourceScope::Used;
        request.section = Some(SectionReadOptions {
            frame_ids: vec!["root-0".into()],
            all_screens: false,
        });
        request.cached_section_index = Some(section_index_with_node_counts(&[8]));
        let mut collector = CollectorSession::new(request);
        let CollectorStep::Call(call) = collector.advance().unwrap() else {
            panic!("first page expected")
        };
        let mut result = boolean_logo_page("root-0", 0);
        let mut envelope: Value =
            serde_json::from_str(result.raw["content"][0]["text"].as_str().unwrap()).unwrap();
        envelope["snapshot"]["nodes"][2]["fields"][field] = value;
        result.raw["content"][0]["text"] = json!(envelope.to_string());
        assert!(
            collector.accept(&call.id, result).is_err(),
            "invalid {field} must not be cached or followed"
        );
    }
}

#[test]
fn failed_section_continuations_do_not_restart_over_accepted_large_values() {
    for malformed_response in [false, true] {
        let mut request = CollectionRequest::new(target("10:1"), CollectionScope::Node);
        request.resource_scope = ResourceScope::Used;
        request.section = Some(SectionReadOptions {
            frame_ids: vec!["root-0".into()],
            all_screens: false,
        });
        request.cached_section_index = Some(section_index_with_node_counts(&[8]));
        let mut collector = CollectorSession::new(request);
        let CollectorStep::Call(first) = collector.advance().unwrap() else {
            panic!("first page expected")
        };
        let mut page = boolean_logo_page("root-0", 0);
        let mut envelope: Value =
            serde_json::from_str(page.raw["content"][0]["text"].as_str().unwrap()).unwrap();
        envelope["snapshot"]["nodes"][0]["fields"]["characters"] = json!({"$largeValue": {
            "nodeId": "root-0", "field": "characters", "byteLength": 19,
            "sha256": "5ab6efd34df9db2f30a0581487fd5d023fde8f658c3dfe9378dbed52332e11f8",
            "cursor": {"nextOffset": 0, "maxChunkBytes": 8}
        }});
        page.raw["content"][0]["text"] = json!(envelope.to_string());
        collector.accept(&first.id, page).unwrap();
        let CollectorStep::Call(large_value) = collector.advance().unwrap() else {
            panic!("large value read expected")
        };
        assert!(matches!(large_value.call, ReadToolCall::LargeValue { .. }));
        let CollectorStep::Call(continuation) = collector.advance().unwrap() else {
            panic!("continuation expected")
        };
        if malformed_response {
            assert!(
                collector
                    .accept(
                        &continuation.id,
                        UpstreamResult {
                            raw: json!({"content": []})
                        }
                    )
                    .is_err()
            );
        } else {
            let error = DevupError::new(
                ErrorCode::DevupFigmaDirectUnavailable,
                "continuation failed",
                true,
            );
            assert!(
                !collector.reject(&continuation.id, &error).unwrap(),
                "propagate the failure instead of restarting at offset zero"
            );
        }
        assert!(
            matches!(collector.advance().unwrap(), CollectorStep::AwaitingResults),
            "no legacy restart may mix with the pending field read"
        );
    }
}

fn legacy_root_id(call: &ReadToolCall) -> &str {
    let ReadToolCall::Snapshot {
        node_id,
        script: BuiltinScript::NodeSnapshot,
        root_ids: None,
        ..
    } = call
    else {
        panic!("legacy root snapshot call expected")
    };
    node_id
}

fn fast_multi_envelope_result(root_ids: &[&str], variable_ids: &[&str]) -> UpstreamResult {
    assert_eq!(root_ids.len(), variable_ids.len());
    let nodes = root_ids
        .iter()
        .zip(variable_ids)
        .map(|(root_id, variable_id)| {
            json!({
                "id": root_id,
                "type": "FRAME",
                "fields": {
                    "name": root_id,
                    "childrenIds": [],
                    "boundVariables": {
                        "fills": [{"type": "VARIABLE_ALIAS", "id": variable_id}]
                    }
                },
                "extra": {},
                "fieldErrors": {}
            })
        })
        .collect::<Vec<_>>();
    let variables = variable_ids
        .iter()
        .map(|id| json!({"id": id, "name": id}))
        .collect::<Vec<_>>();
    let mut envelope = json!({
        "kind": "devupFastSnapshotEnvelope",
        "schemaVersion": 1,
        "source": {"fileKey": "FileKey123", "rootId": "10:1"},
        "snapshot": {
            "fileKey": "FileKey123",
            "version": "v1",
            "rootIds": root_ids,
            "nodes": nodes,
            "diagnostics": []
        },
        "resources": {
            "collections": [],
            "variables": variables,
            "styles": [],
            "usedRemoteVariables": [],
            "usedVariableIds": variable_ids,
            "usedStyleIds": [],
            "localComplete": false,
            "usedRemoteComplete": true,
            "unresolved": []
        },
        "integrity": {
            "nodeCount": root_ids.len(),
            "variableRefCount": variable_ids.len(),
            "styleRefCount": 0,
            "utf8Bytes": 0
        }
    });
    let envelope_bytes = loop {
        let bytes = serde_json::to_vec(&envelope).unwrap();
        if envelope["integrity"]["utf8Bytes"] == bytes.len() as u64 {
            break bytes;
        }
        envelope["integrity"]["utf8Bytes"] = Value::from(bytes.len());
    };
    let _ = envelope_bytes;
    UpstreamResult {
        raw: json!({"content": [
            {"type": "text", "text": envelope.to_string()}
        ]}),
    }
}
