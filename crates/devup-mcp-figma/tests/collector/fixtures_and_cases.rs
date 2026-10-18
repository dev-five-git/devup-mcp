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
    let arguments = fast_call.call.arguments();
    assert!(!arguments.contains_key("nodeId"));
    assert!(
        arguments["code"]
            .as_str()
            .unwrap()
            .contains("devupFastSnapshotEnvelope")
    );
    assert!(
        !arguments["code"]
            .as_str()
            .unwrap()
            .contains("figma.io.write")
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
    assert_eq!(parts.stats.transport, "text");
    assert!(!parts.stats.fallback_used);
    assert_eq!(parts.stats.node_count, 1);
    assert_eq!(parts.stats.variable_count, 0);
    assert_eq!(parts.stats.style_count, 0);
    let metadata = parts.metadata.to_string();
    assert!(!metadata.contains("image/png"));
    assert!(!metadata.contains("devupFastSnapshotDescriptor"));
}

#[test]
fn exact_node_fast_path_accepts_the_stringified_handoff_contract() {
    let mut request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    request.resource_scope = ResourceScope::Used;
    let mut collector = CollectorSession::new(request);

    let CollectorStep::Call(fast_call) = collector.advance().unwrap() else {
        panic!("fast snapshot call expected")
    };
    let result = fast_envelope_result();
    collector
        .accept(
            &fast_call.id,
            UpstreamResult {
                raw: Value::String(result.raw.to_string()),
            },
        )
        .unwrap();

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("stringified fast snapshot should complete without fallback")
    };
    assert_eq!(parts.stats.figma_tool_calls, 1);
    assert_eq!(parts.stats.transport, "text");
    assert!(!parts.stats.fallback_used);
}

/// The about screen as the script really answers it: asked for `mobile`, it
/// gathers `tablet` and `desktop` beside it and pages the three through the
/// fast path. The collector must ride that to the end — it had been reading
/// the family as a target mismatch and restarting a legacy walk that cost
/// 282 calls for a screen the fast path pages through in a handful.
#[test]
fn a_family_gathered_around_the_target_pages_through_the_fast_path() {
    let mut request = CollectionRequest::new(target("1:2"), CollectionScope::Node);
    request.resource_scope = ResourceScope::Used;
    let mut collector = CollectorSession::new(request);

    let CollectorStep::Call(first_call) = collector.advance().unwrap() else {
        panic!("fast snapshot call expected")
    };
    let family = json!(["1:2", "2:2", "3:2"]);
    collector
        .accept(
            &first_call.id,
            family_page(
                &family,
                json!([
                    {"id": "1:2", "type": "FRAME", "fields": {"name": "mobile", "childrenIds": ["1:3"]}, "extra": {}, "fieldErrors": {}},
                    {"id": "2:2", "type": "FRAME", "fields": {"name": "tablet", "childrenIds": []}, "extra": {}, "fieldErrors": {}},
                    {"id": "3:2", "type": "FRAME", "fields": {"name": "desktop", "childrenIds": []}, "extra": {}, "fieldErrors": {}},
                ]),
                (0, 3, false, 4),
            ),
        )
        .unwrap();

    let CollectorStep::Call(second_call) = collector.advance().unwrap() else {
        panic!("the second fast page expected, not a legacy restart")
    };
    assert_eq!(second_call.call.tool_name(), "use_figma");
    let code = second_call.call.arguments()["code"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(code.contains("devupFastSnapshotEnvelope"), "{code}");
    assert!(code.contains("\"offset\":3"), "{code}");
    collector
        .accept(
            &second_call.id,
            family_page(
                &family,
                json!([
                    {"id": "1:3", "type": "TEXT", "fields": {"name": "Child", "characters": "Done", "childrenIds": []}, "extra": {}, "fieldErrors": {}},
                ]),
                (3, 4, true, 4),
            ),
        )
        .unwrap();

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("the family should complete on the fast path")
    };
    assert_eq!(parts.stats.figma_tool_calls, 2);
    assert_eq!(parts.stats.transport, "text-paginated");
    assert!(
        !parts.stats.fallback_used,
        "{:?}",
        parts.stats.fallback_reason
    );
    assert_eq!(parts.stats.node_count, 4);
    assert_eq!(parts.metadata["rootId"], "1:2");
    assert!(
        parts
            .snapshot_chunks
            .iter()
            .all(|chunk| json!(chunk.root_ids) == family),
        "every page is rooted at the family"
    );
}

/// One page of a fast snapshot that the script rooted at `root_ids`.
fn family_page(
    root_ids: &Value,
    nodes: Value,
    (offset, next_offset, complete, total_nodes): (u64, u64, bool, u64),
) -> UpstreamResult {
    let mut nodes = nodes.as_array().cloned().unwrap();
    // The cursor marker is a node of the page and counts in `nodeCount`, as
    // the script counts it.
    let node_count = nodes.len() + 1;
    nodes.push(json!({
        "id": "__DEVUP_SNAPSHOT_CURSOR__",
        "type": "DEVUP_INTERNAL",
        "fields": {"offset": offset, "nextOffset": next_offset, "complete": complete, "totalNodes": total_nodes},
        "extra": {},
        "fieldErrors": {}
    }));
    let mut envelope = json!({
        "kind": "devupFastSnapshotEnvelope",
        "schemaVersion": 1,
        "source": {"fileKey": "FileKey123", "rootId": "1:2"},
        "snapshot": {
            "fileKey": "FileKey123",
            "version": "v1",
            "rootIds": root_ids,
            "nodes": nodes,
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
            "nodeCount": node_count,
            "variableRefCount": 0,
            "styleRefCount": 0,
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
    // The official tool inlines the PNG only when asked, and halves a 1920px
    // screen unless its 1024px cap is raised.
    assert_eq!(
        screenshot_call.call.arguments()["enableBase64Response"],
        true
    );
    assert_eq!(screenshot_call.call.arguments()["maxDimension"], 8192);
    let data = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
    // As the official tool answers, measured 2026-09-07: the URL as JSON in
    // a text block, how to fetch it in another, and the PNG inlined after.
    collector
        .accept(
            &screenshot_call.id,
            UpstreamResult {
                raw: json!({"content": [
                    {"type": "text", "text": "{\"image_url\":\"https://www.figma.com/api/mcp/asset/x.png\",\"width\":1,\"height\":1,\"format\":\"png\"}"},
                    {"type": "text", "text": "The screenshot is hosted at the URL in the first content entry (as JSON)."},
                    {"type": "image", "mimeType": "image/png", "data": data}
                ]}),
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
        Some("textEnvelopeMissing")
    );
    assert_eq!(parts.stats.node_count, 1);
}

#[test]
fn a_split_subtree_still_snapshots_the_node_to_convert() {
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
    collector
        .accept(&metadata_call.id, metadata("FRAME", &["1:3"], 201))
        .unwrap();

    let CollectorStep::Call(child) = collector.advance().unwrap() else {
        panic!("the split child snapshot is expected first")
    };
    let CollectorStep::Call(root) = collector.advance().unwrap() else {
        panic!("the node to convert must be snapshotted as well")
    };
    let child_code = child.call.arguments()["code"].as_str().unwrap().to_owned();
    let root_code = root.call.arguments()["code"].as_str().unwrap().to_owned();
    assert!(child_code.contains("\"1:3\""), "{child_code}");
    assert!(root_code.contains("\"1:2\""), "{root_code}");
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
        "kind": "devupFastSnapshotEnvelope",
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
    let _ = envelope_bytes;
    // No binary transport exists any more: fast snapshots are always plain
    // text. Omitting the `__DEVUP_SNAPSHOT_CURSOR__` marker node is treated
    // by the decoder as a single, already-complete page.
    UpstreamResult {
        raw: json!({"content": [
            {"type": "text", "text": envelope.to_string()}
        ]}),
    }
}

fn fast_theme_envelope_result() -> UpstreamResult {
    let mut envelope = json!({
        "kind": "devupFastThemeEnvelope",
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
    let _ = envelope_bytes;
    UpstreamResult {
        raw: json!({"content": [
            {"type": "text", "text": envelope.to_string()}
        ]}),
    }
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
                "text": "No nodeId was provided. Listing the top-level pages of the document. Call get_metadata again with one of the page ids below (or any node id underneath) to get the XML metadata for that subtree.\n\nTop-level pages of the document:\n- 0:1: Cover\n- 12:34: Body: Proofread"
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
                            "name": "Cover",
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
    // Omitted, not null: the official schema refuses `nodeId: null`.
    assert!(!metadata_call.call.arguments().contains_key("nodeId"));
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
        "name": "Body: Proofread",
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
    let arguments = fast_theme.call.arguments();
    assert!(!arguments.contains_key("nodeId"));
    assert!(
        arguments["code"]
            .as_str()
            .unwrap()
            .contains("devupFastThemeEnvelope")
    );
    collector
        .accept(&fast_theme.id, fast_theme_envelope_result())
        .unwrap();

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("valid fast theme should complete in one call")
    };
    assert_eq!(parts.stats.figma_tool_calls, 1);
    assert_eq!(parts.stats.transport, "text");
    assert!(!parts.stats.fallback_used);
    assert_eq!(parts.stats.variable_count, 1);
    assert_eq!(parts.stats.style_count, 1);
    assert_eq!(parts.variables.as_ref().unwrap().raw["localComplete"], true);
}

/// A file's resources rarely fit one answer, so the theme script pages
/// them: each page carries a run of the resources from an offset and says
/// where the next starts, and the collector asks for the next page from
/// there until one says it is the last. The pages are merged in order, a
/// resource two pages both carry is kept once, and the scan every page
/// repeats comes from the last one.
#[test]
fn a_paged_fast_theme_is_read_page_by_page_and_merged() {
    let mut request = CollectionRequest::new(file_target(), CollectionScope::File);
    request.resource_scope = ResourceScope::File;
    request.variables_only = true;
    let mut collector = CollectorSession::new(request);

    let CollectorStep::Call(first) = collector.advance().unwrap() else {
        panic!("fast theme call expected")
    };
    assert!(
        first.call.arguments()["code"]
            .as_str()
            .unwrap()
            .contains("{\"offset\":0}")
    );
    collector
        .accept(
            &first.id,
            fast_theme_page_result(
                json!({"collections": [{"id": "c", "name": "Theme"}], "variables": [{"id": "v1", "name": "primary"}], "styles": [], "usedVariableIds": ["v1"]}),
                json!({"offset": 0, "nextOffset": 2, "complete": false, "totalItems": 6}),
            ),
        )
        .unwrap();

    let CollectorStep::Call(second) = collector.advance().unwrap() else {
        panic!("the next page is asked for")
    };
    assert_eq!(second.call.tool_name(), "use_figma");
    assert!(
        second.call.arguments()["code"]
            .as_str()
            .unwrap()
            .contains("{\"offset\":2}")
    );
    collector
        .accept(
            &second.id,
            fast_theme_page_result(
                json!({"collections": [], "variables": [{"id": "v1", "name": "primary"}, {"id": "v2", "name": "text"}], "styles": [{"id": "s", "name": "body", "styleType": "TEXT"}], "usedVariableIds": ["v1", "v2"], "usedStyleIds": ["s"]}),
                json!({"offset": 2, "nextOffset": 6, "complete": true, "totalItems": 6}),
            ),
        )
        .unwrap();

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("the last page completes the theme")
    };
    assert_eq!(parts.stats.figma_tool_calls, 2);
    assert_eq!(parts.stats.transport, "text");
    assert!(!parts.stats.fallback_used);
    let resources = &parts.variables.as_ref().unwrap().raw;
    assert_eq!(resources["collections"].as_array().unwrap().len(), 1);
    // v1 was on both pages and is kept once.
    assert_eq!(
        resources["variables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|variable| variable["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["v1", "v2"]
    );
    assert_eq!(resources["styles"].as_array().unwrap().len(), 1);
    assert_eq!(resources["localComplete"], true);
    // The scan came in two shares, v1 on both, and is whole and unrepeated.
    assert_eq!(resources["usedVariableIds"], json!(["v1", "v2"]));
    assert_eq!(resources["usedStyleIds"], json!(["s"]));
    assert_eq!(parts.stats.variable_count, 2);
    assert_eq!(parts.stats.style_count, 1);
    assert_eq!(parts.metadata["pageCount"], 2);
}