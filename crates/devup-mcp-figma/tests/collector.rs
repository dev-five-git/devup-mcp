use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp_figma::{
    BuiltinScript, CollectionRequest, CollectionScope, CollectorSession, CollectorStep, DevupError,
    DiagnosticSeverity, ErrorCode, ExploreBounds, ExploreReadOptions, FigmaTarget, ReadToolCall,
    ResourceScope, SectionCandidate, SectionIndex, SectionReadOptions, SectionSummary,
    UpstreamResult, merge_chunks,
};
use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
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
fn requested_reference_png_is_collected_after_the_design_snapshot() {
    let mut request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    request.resource_scope = ResourceScope::Used;
    request.reference_png = true;
    let mut collector = CollectorSession::new(request);

    let CollectorStep::Call(fast_call) = collector.advance().unwrap() else {
        panic!("fast snapshot call expected")
    };
    collector
        .accept(&fast_call.id, fast_envelope_result())
        .unwrap();

    let CollectorStep::Call(screenshot_call) = collector.advance().unwrap() else {
        panic!("reference screenshot call expected")
    };
    assert_eq!(screenshot_call.call.tool_name(), "get_screenshot");
    assert_eq!(screenshot_call.call.arguments()["fileKey"], "FileKey123");
    assert_eq!(screenshot_call.call.arguments()["nodeId"], "1:2");
    let data = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
    collector
        .accept(
            &screenshot_call.id,
            UpstreamResult {
                raw: json!({"content": [{"type": "image", "mimeType": "image/png", "data": data}]}),
            },
        )
        .unwrap();

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("collection should complete with its reference PNG")
    };
    let reference = parts.reference_png.expect("reference PNG");
    assert_eq!(reference.mime_type, "image/png");
    assert_eq!(reference.data_base64, data);
    assert_eq!(reference.byte_length, STANDARD.decode(data).unwrap().len());
    assert_eq!(reference.sha256.len(), 64);
    assert_eq!(parts.stats.figma_tool_calls, 2);
}

#[test]
fn reference_png_rejects_a_signature_only_payload() {
    let (mut collector, screenshot_call_id) = collector_awaiting_reference_png();
    let signature_only = STANDARD.encode(b"\x89PNG\r\n\x1a\n");

    let error = collector
        .accept(
            &screenshot_call_id,
            screenshot_result(json!({
                "type": "image",
                "mimeType": "image/png",
                "data": signature_only
            })),
        )
        .expect_err("a PNG signature without chunks or pixels must be rejected");

    assert_eq!(error.code, ErrorCode::DevupFigmaHandoffInvalid);
}

#[test]
fn reference_png_rejects_a_truncated_image() {
    let (mut collector, screenshot_call_id) = collector_awaiting_reference_png();
    let mut png = STANDARD.decode(valid_reference_png_base64()).unwrap();
    png.truncate(png.len() - 8);

    let error = collector
        .accept(
            &screenshot_call_id,
            screenshot_result(json!({
                "type": "image",
                "mimeType": "image/png",
                "data": STANDARD.encode(png)
            })),
        )
        .expect_err("a truncated PNG must be rejected even when its signature is intact");

    assert_eq!(error.code, ErrorCode::DevupFigmaHandoffInvalid);
}

#[test]
fn reference_png_rejects_multiple_image_content_blocks() {
    let (mut collector, screenshot_call_id) = collector_awaiting_reference_png();
    let image = json!({
        "type": "image",
        "mimeType": "image/png",
        "data": valid_reference_png_base64()
    });

    let error = collector
        .accept(
            &screenshot_call_id,
            UpstreamResult {
                raw: json!({"content": [image.clone(), image]}),
            },
        )
        .expect_err("referencePng accepts exactly one official ImageContent block");

    assert_eq!(error.code, ErrorCode::DevupFigmaHandoffInvalid);
}

#[test]
fn reference_png_rejects_zero_content_blocks() {
    let (mut collector, screenshot_call_id) = collector_awaiting_reference_png();

    let error = collector
        .accept(
            &screenshot_call_id,
            UpstreamResult {
                raw: json!({"content": []}),
            },
        )
        .expect_err("referencePng requires exactly one ImageContent block");

    assert_eq!(error.code, ErrorCode::DevupFigmaHandoffInvalid);
}

#[test]
fn reference_png_rejects_an_image_hidden_in_json_text() {
    let (mut collector, screenshot_call_id) = collector_awaiting_reference_png();
    let hidden = json!({
        "type": "image",
        "mimeType": "image/png",
        "data": valid_reference_png_base64()
    })
    .to_string();

    let error = collector
        .accept(
            &screenshot_call_id,
            screenshot_result(json!({"type": "text", "text": hidden})),
        )
        .expect_err("JSON embedded in text is not an official ImageContent response");

    assert_eq!(error.code, ErrorCode::DevupFigmaHandoffInvalid);
}

#[test]
fn reference_png_rejects_an_image_nested_outside_the_official_result() {
    let (mut collector, screenshot_call_id) = collector_awaiting_reference_png();

    let error = collector
        .accept(
            &screenshot_call_id,
            UpstreamResult {
                raw: json!({
                    "result": {
                        "content": [{
                            "type": "image",
                            "mimeType": "image/png",
                            "data": valid_reference_png_base64()
                        }]
                    }
                }),
            },
        )
        .expect_err("nested lookalikes are not the official CallToolResult shape");

    assert_eq!(error.code, ErrorCode::DevupFigmaHandoffInvalid);
}

#[test]
fn reference_png_rejects_dimensions_above_the_decode_bound() {
    let (mut collector, screenshot_call_id) = collector_awaiting_reference_png();
    let pixels = vec![0_u8; 8_193];
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(&pixels, 8_193, 1, ExtendedColorType::L8)
        .unwrap();

    let error = collector
        .accept(
            &screenshot_call_id,
            screenshot_result(json!({
                "type": "image",
                "mimeType": "image/png",
                "data": STANDARD.encode(png)
            })),
        )
        .expect_err("decoded dimensions are bounded independently of compressed size");

    assert_eq!(error.code, ErrorCode::DevupFigmaResponseTooLarge);
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

fn collector_awaiting_reference_png() -> (CollectorSession, String) {
    let mut request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    request.resource_scope = ResourceScope::Used;
    request.reference_png = true;
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(fast_call) = collector.advance().unwrap() else {
        panic!("fast snapshot call expected")
    };
    collector
        .accept(&fast_call.id, fast_envelope_result())
        .unwrap();
    let CollectorStep::Call(screenshot_call) = collector.advance().unwrap() else {
        panic!("reference screenshot call expected")
    };
    (collector, screenshot_call.id)
}

fn screenshot_result(content: Value) -> UpstreamResult {
    UpstreamResult {
        raw: json!({"content": [content]}),
    }
}

fn valid_reference_png_base64() -> &'static str {
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="
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

fn fast_theme_envelope_result() -> UpstreamResult {
    let mut envelope = json!({
        "schemaVersion": 1,
        "source": {"fileKey": "FileKey123", "version": "v2"},
        "resources": {
            "collections": [{"id": "c", "name": "Theme"}],
            "variables": [{"id": "v", "name": "primary"}],
            "styles": [{"id": "s", "name": "body", "styleType": "TEXT"}],
            "usedRemoteVariables": [],
            "usedVariableIds": ["v"],
            "usedStyleIds": ["s"],
            "localComplete": true,
            "usedRemoteComplete": true,
            "unresolved": []
        },
        "integrity": {
            "collectionCount": 1,
            "variableCount": 1,
            "styleCount": 1,
            "unresolvedCount": 0,
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
        "kind": "devupFastThemeDescriptor",
        "schemaVersion": 1,
        "collectionCount": 1,
        "variableCount": 1,
        "styleCount": 1,
        "unresolvedCount": 0,
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
    let CollectorStep::Call(fast_theme) = collector.advance().unwrap() else {
        panic!("fast theme call expected")
    };
    assert_eq!(fast_theme.call.tool_name(), "use_figma");
    assert!(
        fast_theme.call.arguments()["code"]
            .as_str()
            .unwrap()
            .contains("devupFastThemeDescriptor")
    );
    collector
        .accept(&fast_theme.id, fast_theme_envelope_result())
        .unwrap();

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("valid fast theme should complete in one call")
    };
    assert_eq!(parts.stats.figma_tool_calls, 1);
    assert_eq!(parts.stats.transport, "png-theme-envelope-v1");
    assert!(!parts.stats.fallback_used);
    assert_eq!(parts.stats.variable_count, 1);
    assert_eq!(parts.stats.style_count, 1);
    assert_eq!(parts.variables.as_ref().unwrap().raw["localComplete"], true);
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

    let CollectorStep::Call(batch_call) = collector.advance().unwrap() else {
        panic!("one bounded multi-root call expected")
    };
    let arguments = batch_call.call.arguments();
    let code = arguments["code"].as_str().unwrap();
    assert!(code.contains("[\"10:3\",\"10:2\"]"));
    assert_eq!(batch_call.expected_node_id.as_deref(), Some("10:1"));
}

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
        "rootId": "10:1",
        "nodeCount": root_ids.len(),
        "variableRefCount": variable_ids.len(),
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
