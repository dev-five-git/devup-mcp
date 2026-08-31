use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp_figma::{
    CollectionRequest, CollectionScope, CollectorSession, CollectorStep, DevupError,
    DiagnosticSeverity, ErrorCode, ExploreReadOptions, FigmaTarget, ResourceScope, UpstreamResult,
};
use serde_json::{Value, json};

#[test]
fn exact_node_fast_path_completes_in_one_call() {
    let mut request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    request.resource_scope = ResourceScope::Used;
    let mut collector = CollectorSession::new(request);

    let CollectorStep::Call(fast_call) = collector.advance().unwrap() else {
        panic!("fast snapshot call expected")
    };
    assert_eq!(fast_call.call.tool_name(), "use_figma");
    assert!(
        fast_call.call.arguments()["code"]
            .as_str()
            .unwrap()
            .contains("devupFastSnapshotDescriptor")
    );

    collector
        .accept(&fast_call.id, fast_envelope_result())
        .unwrap();
    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("fast snapshot should complete without another call")
    };

    assert_eq!(parts.snapshot_chunks.len(), 1);
    assert_eq!(parts.snapshot_chunks[0].nodes.len(), 1);
    assert_eq!(parts.stats.figma_tool_calls, 1);
    assert_eq!(parts.stats.transport, "png-envelope-v1");
    assert!(!parts.stats.fallback_used);
    assert_eq!(parts.stats.node_count, 1);
    assert_eq!(parts.stats.variable_count, 0);
    assert_eq!(parts.stats.style_count, 0);
    let metadata = parts.metadata.to_string();
    assert!(!metadata.contains("image/png"));
    assert!(!metadata.contains("devupFastSnapshotDescriptor"));
}

#[test]
fn malformed_fast_result_restarts_legacy_from_metadata() {
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
                raw: json!({"content": [{"type": "text", "text": "unsupported"}]}),
            },
        )
        .unwrap();

    let CollectorStep::Call(metadata_call) = collector.advance().unwrap() else {
        panic!("legacy metadata must restart after a malformed envelope")
    };
    assert_eq!(metadata_call.call.tool_name(), "get_metadata");
    collector
        .accept(&metadata_call.id, metadata("FRAME", &[], 1))
        .unwrap();
    let CollectorStep::Call(snapshot_call) = collector.advance().unwrap() else {
        panic!("legacy snapshot expected")
    };
    assert!(
        snapshot_call.call.arguments()["code"]
            .as_str()
            .unwrap()
            .contains("__DEVUP_SNAPSHOT_CURSOR__")
    );
    collector
        .accept(&snapshot_call.id, snapshot("1:2"))
        .unwrap();

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("legacy fallback should complete")
    };
    assert_eq!(parts.stats.figma_tool_calls, 3);
    assert_eq!(parts.stats.transport, "legacy-cursor");
    assert!(parts.stats.fallback_used);
    assert_eq!(
        parts.stats.fallback_reason.as_deref(),
        Some("descriptorMissing")
    );
    assert_eq!(parts.stats.node_count, 1);
}

#[test]
fn rejected_fast_call_can_restart_legacy_collection() {
    let mut request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    request.resource_scope = ResourceScope::Used;
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(fast_call) = collector.advance().unwrap() else {
        panic!("fast call expected")
    };

    let recovered = collector
        .reject(
            &fast_call.id,
            &DevupError::new(
                ErrorCode::DevupFigmaDirectUnavailable,
                "private upstream detail",
                true,
            ),
        )
        .unwrap();

    assert!(recovered);
    let CollectorStep::Call(metadata_call) = collector.advance().unwrap() else {
        panic!("legacy metadata call expected")
    };
    assert_eq!(metadata_call.call.tool_name(), "get_metadata");
}

#[test]
fn legacy_used_resources_combine_variables_and_styles_in_one_call() {
    let mut request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    request.resource_scope = ResourceScope::Used;
    let mut collector = CollectorSession::new(request);

    let CollectorStep::Call(fast_call) = collector.advance().unwrap() else {
        panic!()
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
        panic!()
    };
    collector
        .accept(&metadata_call.id, metadata("FRAME", &[], 1))
        .unwrap();
    let CollectorStep::Call(snapshot_call) = collector.advance().unwrap() else {
        panic!()
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

    let CollectorStep::Call(resources_call) = collector.advance().unwrap() else {
        panic!("one combined resource call expected")
    };
    let code = resources_call.call.arguments()["code"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(code.contains("VariableID:1:2"));
    assert!(code.contains("S:text"));
    assert!(matches!(
        collector.advance().unwrap(),
        CollectorStep::AwaitingResults
    ));
}

fn target(node_id: &str) -> FigmaTarget {
    FigmaTarget::parse(&format!(
        "https://www.figma.com/design/FileKey123/Fixture?node-id={}",
        node_id.replace(':', "-")
    ))
    .unwrap()
}

fn fast_envelope_result() -> UpstreamResult {
    let mut envelope = json!({
        "schemaVersion": 1,
        "source": {"fileKey": "FileKey123", "rootId": "1:2"},
        "snapshot": {
            "fileKey": "FileKey123",
            "version": "v1",
            "rootIds": ["1:2"],
            "nodes": [{
                "id": "1:2",
                "type": "FRAME",
                "fields": {"name": "Synthetic", "childrenIds": []},
                "extra": {},
                "fieldErrors": {}
            }],
            "diagnostics": []
        },
        "resources": {
            "collections": [],
            "variables": [],
            "styles": [],
            "usedRemoteVariables": [],
            "localComplete": false,
            "usedRemoteComplete": true,
            "unresolved": []
        },
        "integrity": {
            "nodeCount": 1,
            "variableRefCount": 0,
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

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    push_png_chunk(&mut png, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
    let mut payload = Vec::with_capacity(envelope_bytes.len() + 8);
    payload.extend_from_slice(&0_u32.to_be_bytes());
    payload.extend_from_slice(&1_u32.to_be_bytes());
    payload.extend_from_slice(&envelope_bytes);
    push_png_chunk(&mut png, b"duVp", &payload);
    push_png_chunk(
        &mut png,
        b"IDAT",
        &[
            0x78, 0x01, 0x01, 0x05, 0x00, 0xfa, 0xff, 0, 0, 0, 0, 0, 5, 0, 1,
        ],
    );
    push_png_chunk(&mut png, b"IEND", &[]);
    let descriptor = json!({
        "kind": "devupFastSnapshotDescriptor",
        "schemaVersion": 1,
        "rootId": "1:2",
        "nodeCount": 1,
        "variableRefCount": 0,
        "styleRefCount": 0,
        "utf8Bytes": envelope_bytes.len(),
        "chunkCount": 1
    });
    UpstreamResult {
        raw: json!({"content": [
            {"type": "text", "text": descriptor.to_string()},
            {"type": "image", "data": STANDARD.encode(png), "mimeType": "image/png"}
        ]}),
    }
}

fn push_png_chunk(output: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    output.extend_from_slice(&(data.len() as u32).to_be_bytes());
    output.extend_from_slice(chunk_type);
    output.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(chunk_type);
    crc_input.extend_from_slice(data);
    output.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn file_target() -> FigmaTarget {
    FigmaTarget::parse("https://www.figma.com/design/FileKey123/Fixture").unwrap()
}

fn metadata(node_type: &str, children: &[&str], descendant_count: usize) -> UpstreamResult {
    UpstreamResult {
        raw: json!({
            "structuredContent": {
                "devupMetadata": {
                    "fileKey": "FileKey123",
                    "version": "v1",
                    "rootId": "1:2",
                    "nodes": [{
                        "id": "1:2",
                        "type": node_type,
                        "childrenIds": children,
                        "descendantCount": descendant_count
                    }]
                }
            }
        }),
    }
}

fn snapshot(root_id: &str) -> UpstreamResult {
    UpstreamResult {
        raw: json!({
            "fileKey": "FileKey123",
            "version": "v1",
            "rootIds": [root_id],
            "nodes": [{
                "id": root_id,
                "type": "FRAME",
                "fields": {"name": "Synthetic"},
                "extra": {},
                "fieldErrors": {}
            }]
        }),
    }
}

fn official_xml_metadata() -> UpstreamResult {
    UpstreamResult {
        raw: json!({
            "content": [
                {
                    "type": "text",
                    "text": "<frame id=\"1:2\" name=\"Synthetic Root\" x=\"0\" y=\"0\" width=\"320\" height=\"240\"><text id=\"1:3\" name=\"Synthetic Child\" x=\"8\" y=\"8\" width=\"100\" height=\"20\" /></frame>"
                },
                {
                    "type": "text",
                    "text": "Synthetic guidance that is not metadata"
                }
            ]
        }),
    }
}

fn official_top_level_pages() -> UpstreamResult {
    UpstreamResult {
        raw: json!({
            "content": [{
                "type": "text",
                "text": "No nodeId was provided. Listing the top-level pages of the document. Call get_metadata again with one of the page ids below (or any node id underneath) to get the XML metadata for that subtree.\n\nTop-level pages of the document:\n- 0:1: 표지\n- 12:34: 본문: 교정"
            }]
        }),
    }
}

fn file_page_metadata() -> UpstreamResult {
    UpstreamResult {
        raw: json!({
            "structuredContent": {
                "devupMetadata": {
                    "fileKey": "FileKey123",
                    "version": "v1",
                    "rootId": "0:1",
                    "nodes": [
                        {
                            "id": "0:1",
                            "type": "PAGE",
                            "name": "표지",
                            "childrenIds": ["1:2"],
                            "descendantCount": 1
                        },
                        {
                            "id": "1:2",
                            "type": "FRAME",
                            "name": "A : STORY-F-PROOFREAD",
                            "childrenIds": [],
                            "descendantCount": 0
                        }
                    ]
                }
            }
        }),
    }
}

#[test]
fn file_scope_starts_from_the_file_even_when_the_url_contains_a_node() {
    let request = CollectionRequest::new(target("1:2"), CollectionScope::File);
    let mut collector = CollectorSession::new(request);

    let CollectorStep::Call(metadata_call) = collector.advance().unwrap() else {
        panic!("metadata call expected")
    };
    assert_eq!(metadata_call.call.tool_name(), "get_metadata");
    assert_eq!(metadata_call.expected_node_id, None);
    assert_eq!(metadata_call.call.arguments()["nodeId"], json!(null));
}

#[test]
fn official_top_level_page_list_expands_to_page_metadata_calls() {
    let request = CollectionRequest::new(file_target(), CollectionScope::File);
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(file_metadata) = collector.advance().unwrap() else {
        panic!("file metadata call expected")
    };

    collector
        .accept(&file_metadata.id, official_top_level_pages())
        .unwrap();

    let CollectorStep::Call(first_page) = collector.advance().unwrap() else {
        panic!("first page metadata call expected")
    };
    let CollectorStep::Call(second_page) = collector.advance().unwrap() else {
        panic!("second page metadata call expected")
    };
    assert_eq!(first_page.call.tool_name(), "get_metadata");
    assert_eq!(first_page.expected_node_id.as_deref(), Some("0:1"));
    assert_eq!(second_page.call.tool_name(), "get_metadata");
    assert_eq!(second_page.expected_node_id.as_deref(), Some("12:34"));
}

#[test]
fn metadata_only_file_collection_completes_without_snapshot_calls() {
    let mut request = CollectionRequest::new(file_target(), CollectionScope::File);
    request.metadata_only = true;
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(file_metadata) = collector.advance().unwrap() else {
        panic!("file metadata call expected")
    };
    collector
        .accept(&file_metadata.id, official_top_level_pages())
        .unwrap();

    let CollectorStep::Call(first_page) = collector.advance().unwrap() else {
        panic!("first page metadata call expected")
    };
    let CollectorStep::Call(second_page) = collector.advance().unwrap() else {
        panic!("second page metadata call expected")
    };
    collector
        .accept(&first_page.id, file_page_metadata())
        .unwrap();
    let mut second = file_page_metadata();
    second.raw["structuredContent"]["devupMetadata"]["rootId"] = json!("12:34");
    second.raw["structuredContent"]["devupMetadata"]["nodes"] = json!([{
        "id": "12:34",
        "type": "PAGE",
        "name": "본문: 교정",
        "childrenIds": [],
        "descendantCount": 0
    }]);
    collector.accept(&second_page.id, second).unwrap();

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("metadata-only collection should complete")
    };
    assert_eq!(parts.snapshot_chunks.len(), 1);
    let chunk = &parts.snapshot_chunks[0];
    assert_eq!(chunk.root_ids, ["0:1", "12:34"]);
    assert_eq!(chunk.nodes.len(), 3);
    let match_node = chunk.nodes.iter().find(|node| node.id == "1:2").unwrap();
    assert_eq!(match_node.fields["name"], json!("A : STORY-F-PROOFREAD"));
}

#[test]
fn variables_only_file_collection_skips_page_and_node_snapshots() {
    let mut request = CollectionRequest::new(file_target(), CollectionScope::File);
    request.resource_scope = ResourceScope::File;
    request.variables_only = true;
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(file_metadata) = collector.advance().unwrap() else {
        panic!("file metadata call expected")
    };
    collector
        .accept(&file_metadata.id, official_top_level_pages())
        .unwrap();

    let CollectorStep::Call(variable_catalog) = collector.advance().unwrap() else {
        panic!("variable catalog should immediately follow page discovery")
    };
    assert_eq!(variable_catalog.call.tool_name(), "use_figma");
    assert!(
        variable_catalog.call.arguments()["code"]
            .as_str()
            .unwrap()
            .contains("getLocalVariableCollectionsAsync")
    );
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
                    "variables": [{"id": "VariableID:1:2", "name": "primary", "remote": true}],
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
    assert_eq!(resources["variables"][0]["name"], "primary");
    assert_eq!(resources["styles"], json!([]));
    assert_eq!(resources["usedRemoteComplete"], false);
    assert_eq!(resources["unresolved"][0]["id"], "S:text");
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
                        {"id": "__DEVUP_SNAPSHOT_CURSOR__", "type": "DEVUP_INTERNAL", "fields": {"nextOffset": 1, "complete": false, "totalNodes": 2}, "extra": {}, "fieldErrors": {}}
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
                        {"id": "1:3", "type": "TEXT", "fields": {"name": "Child", "characters": "완료", "childrenIds": []}, "extra": {}, "fieldErrors": {}},
                        {"id": "__DEVUP_SNAPSHOT_CURSOR__", "type": "DEVUP_INTERNAL", "fields": {"nextOffset": 2, "complete": true, "totalNodes": 2}, "extra": {}, "fieldErrors": {}}
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
