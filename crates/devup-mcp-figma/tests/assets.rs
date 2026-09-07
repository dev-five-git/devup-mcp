use std::collections::BTreeMap;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp_figma::{
    AssetExportOutcome, AssetFormat, AssetRequest, AssetSelection, AssetStatus, CollectionRequest,
    CollectionScope, CollectorSession, CollectorStep, FigmaTarget, RawNode, ReadToolCall, Snapshot,
    UpstreamResult, asset_export_from_result, discover_asset_manifest, validate_asset_requests,
};
use serde_json::{Map, json};
use sha2::Digest as _;

fn node(id: &str, node_type: &str, fields: serde_json::Value) -> RawNode {
    RawNode {
        id: id.to_owned(),
        node_type: node_type.to_owned(),
        fields: serde_json::from_value(fields).unwrap(),
        extra: Map::new(),
        field_errors: BTreeMap::new(),
    }
}

#[test]
fn collector_exports_only_explicit_assets_and_preserves_snapshot_on_export_failure() {
    let target =
        FigmaTarget::parse("https://www.figma.com/design/FileKey123/Fixture?node-id=1-1").unwrap();
    let mut request = CollectionRequest::new(target, CollectionScope::Node);
    request.asset_selections = vec![AssetSelection {
        asset_id: "1:1:fills:0".to_owned(),
        format: AssetFormat::Png,
        scale: 2,
    }];
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(metadata) = collector.advance().unwrap() else {
        panic!("metadata")
    };
    collector
        .accept(
            &metadata.id,
            UpstreamResult {
                raw: json!({"structuredContent":{"devupMetadata":{
                    "fileKey":"FileKey123","version":"v1","rootId":"1:1",
                    "nodes":[{"id":"1:1","type":"FRAME","childrenIds":[],"descendantCount":0}]
                }}}),
            },
        )
        .unwrap();
    let CollectorStep::Call(snapshot_call) = collector.advance().unwrap() else {
        panic!("snapshot")
    };
    collector
        .accept(
            &snapshot_call.id,
            UpstreamResult {
                raw: json!({
                    "fileKey":"FileKey123","version":"v1","rootIds":["1:1"],
                    "nodes":[
                        serde_json::to_value(snapshot().nodes["1:1"].clone()).unwrap(),
                        {"id":"__DEVUP_SNAPSHOT_CURSOR__","type":"DEVUP_INTERNAL","fields":{"offset":0,"nextOffset":1,"complete":true,"totalNodes":1},"extra":{},"fieldErrors":{}}
                    ],"diagnostics":[]
                }),
            },
        )
        .unwrap();

    let CollectorStep::Call(asset_call) = collector.advance().unwrap() else {
        panic!("explicit asset export")
    };
    let ReadToolCall::AssetExport { request, .. } = asset_call.call else {
        panic!("asset export call")
    };
    assert_eq!(request.asset_id, "1:1:fills:0");
    collector
        .accept(
            &asset_call.id,
            UpstreamResult {
                raw: json!({
                    "kind":"devupAssetExport","fileKey":"FileKey123","version":"v1",
                    "assetId":"1:1:fills:0","nodeId":"1:1","field":"fills/0",
                    "imageHash":"image-hash-123","format":"png","scale":2,
                    "status":"failed","byteLength":null,"sha256":null,
                    "errorCode":"DEVUP_ASSET_EXPORT_FAILED"
                }),
            },
        )
        .unwrap();
    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("complete")
    };
    assert_eq!(parts.assets.len(), 1);
    assert_eq!(parts.assets[0].status, AssetStatus::Failed);
    assert_eq!(parts.snapshot_chunks[0].nodes.len(), 1);
    assert!(
        parts.snapshot_chunks[0]
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "DEVUP_ASSET_EXPORT_FAILED")
    );
}

/// Figma's remote MCP returns a written PNG as an image attachment but does
/// not return a written `.svg` at all — the response carries only the
/// descriptor, as JSON inside a text block. So an SVG export inlines its own
/// payload beside the descriptor, and the payload search has to step through
/// that JSON encoding to reach it. Before this, every SVG request failed with
/// "asset export response does not contain the requested binary" while PNG
/// worked, and the error said nothing about why.
#[test]
fn an_svg_payload_inlined_beside_the_descriptor_is_decoded_from_its_text() {
    let svg = "<svg width=\"2\" height=\"2\" xmlns=\"http://www.w3.org/2000/svg\"></svg>";
    let bytes = svg.as_bytes();
    let sha256: String = sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let descriptor = json!({
        "kind": "devupAssetExport", "fileKey": "FileKey123", "version": "v1",
        "assetId": "1:2:node", "nodeId": "1:2", "field": "node",
        "imageHash": null, "format": "svg", "scale": 1,
        "status": "exported", "byteLength": bytes.len(), "sha256": sha256,
        "mimeType": "image/svg+xml", "text": svg, "errorCode": null
    });
    // Exactly how it arrives: the descriptor serialized into a text block.
    let result = UpstreamResult {
        raw: json!({"content": [{"type": "text", "text": descriptor.to_string()}]}),
    };
    let request = AssetRequest {
        asset_id: "1:2:node".to_owned(),
        node_id: "1:2".to_owned(),
        field: "node".to_owned(),
        image_hash: None,
        format: AssetFormat::Svg,
        scale: 1,
    };

    let AssetExportOutcome::Entry(entry) =
        asset_export_from_result(&result, "FileKey123", Some("v1"), &request)
            .expect("an inlined SVG payload must decode")
    else {
        panic!("an SVG under the inline cap is exported in one answer")
    };

    assert_eq!(entry.status, AssetStatus::Exported);
    assert_eq!(entry.byte_length, Some(bytes.len()));
    assert_eq!(entry.mime_type.as_deref(), Some("image/svg+xml"));
    // Re-encoded to base64 so every consumer downstream is shape-independent.
    let decoded = STANDARD
        .decode(entry.data_base64.expect("payload").as_bytes())
        .expect("base64");
    assert_eq!(decoded, bytes);
}

/// A response that carries no payload at all must say what it *did* carry,
/// so "nothing came back", "wrong mime type" and "unread field" stay
/// distinguishable instead of collapsing into one opaque sentence.
#[test]
fn a_missing_asset_payload_reports_the_shapes_that_were_present() {
    let descriptor = json!({
        "kind": "devupAssetExport", "fileKey": "FileKey123", "version": "v1",
        "assetId": "1:2:node", "nodeId": "1:2", "field": "node",
        "imageHash": null, "format": "svg", "scale": 1,
        "status": "exported", "byteLength": 10, "sha256": "00", "errorCode": null
    });
    let result = UpstreamResult {
        raw: json!({"content": [{"type": "text", "text": descriptor.to_string()}]}),
    };
    let request = AssetRequest {
        asset_id: "1:2:node".to_owned(),
        node_id: "1:2".to_owned(),
        field: "node".to_owned(),
        image_hash: None,
        format: AssetFormat::Svg,
        scale: 1,
    };

    let error = asset_export_from_result(&result, "FileKey123", Some("v1"), &request)
        .expect_err("no payload is an error");

    assert_eq!(error.details["expectedMimeType"], "image/svg+xml");
    let observed = error.details["observed"].as_array().expect("observed");
    assert!(
        observed.iter().any(|entry| entry
            .as_str()
            .unwrap_or_default()
            .contains("carries=[text]")),
        "the diagnostic must name the shapes that were present: {observed:?}"
    );
}

fn snapshot() -> Snapshot {
    Snapshot {
        file_key: "FileKey123".to_owned(),
        version: Some("v1".to_owned()),
        roots: vec!["1:1".to_owned()],
        nodes: [node(
            "1:1",
            "FRAME",
            json!({
                "childrenIds": [],
                "isAsset": true,
                "fills": [{"type": "IMAGE", "imageHash": "image-hash-123", "scaleMode": "FILL"}]
            }),
        )]
        .into_iter()
        .map(|node| (node.id.clone(), node))
        .collect(),
        diagnostics: Vec::new(),
    }
}

#[test]
fn manifest_preserves_image_and_vector_source_details_without_exporting_bytes() {
    let manifest = discover_asset_manifest(&snapshot());

    assert_eq!(manifest.version, 1);
    assert_eq!(manifest.assets.len(), 1);
    assert_eq!(manifest.assets[0].asset_id, "1:1:fills:0");
    assert_eq!(manifest.assets[0].node_id, "1:1");
    assert_eq!(manifest.assets[0].field, "fills/0");
    assert_eq!(manifest.assets[0].source_kind, "image-fill");
    assert_eq!(
        manifest.assets[0].image_hash.as_deref(),
        Some("image-hash-123")
    );
    assert_eq!(manifest.assets[0].status, AssetStatus::Available);
    assert!(manifest.assets[0].data_base64.is_none());
}

fn hidden_asset_snapshot() -> Snapshot {
    Snapshot {
        file_key: "FileKey123".to_owned(),
        version: Some("v1".to_owned()),
        roots: vec!["1:1".to_owned()],
        nodes: [node(
            "1:1",
            "FRAME",
            json!({
                "childrenIds": [],
                "visible": false,
                "isAsset": true,
                "fills": [{"type": "IMAGE", "imageHash": "image-hash-123", "scaleMode": "FILL"}]
            }),
        )]
        .into_iter()
        .map(|node| (node.id.clone(), node))
        .collect(),
        diagnostics: Vec::new(),
    }
}

#[test]
fn hidden_node_is_reported_as_unexportable_instead_of_available() {
    let manifest = discover_asset_manifest(&hidden_asset_snapshot());

    assert_eq!(manifest.assets.len(), 1);
    assert_eq!(manifest.assets[0].status, AssetStatus::Failed);
    assert_eq!(
        manifest.assets[0].error_code.as_deref(),
        Some("DEVUP_ASSET_NODE_HIDDEN")
    );
}

#[test]
fn requesting_a_hidden_asset_is_rejected_with_the_reason() {
    let error = validate_asset_requests(
        &hidden_asset_snapshot(),
        &[AssetRequest {
            asset_id: "1:1:fills:0".to_owned(),
            node_id: "1:1".to_owned(),
            field: "fills/0".to_owned(),
            image_hash: Some("image-hash-123".to_owned()),
            format: AssetFormat::Png,
            scale: 1,
        }],
    )
    .expect_err("a hidden node cannot be exported, so the request must be refused");

    assert!(format!("{error:?}").contains("hidden"), "{error:?}");
}

fn manifest_for(roots: &[&str], nodes: Vec<RawNode>) -> devup_mcp_figma::AssetManifest {
    discover_asset_manifest(&Snapshot {
        file_key: "FileKey123".to_owned(),
        version: Some("v1".to_owned()),
        roots: roots.iter().map(|root| (*root).to_owned()).collect(),
        nodes: nodes
            .into_iter()
            .map(|node| (node.id.clone(), node))
            .collect(),
        diagnostics: Vec::new(),
    })
}

#[test]
fn icon_container_wins_over_its_vector_fragments() {
    let manifest = manifest_for(
        &["3997:46297"],
        vec![
            node(
                "3997:46297",
                "FRAME",
                json!({"name": "input", "childrenIds": ["3997:46298", "3997:46301"]}),
            ),
            node(
                "3997:46298",
                "FRAME",
                json!({
                    "name": "kakao-talk_2111496 1",
                    "parentId": "3997:46297",
                    "isAsset": true,
                    "childrenIds": ["3997:46299", "3997:46300"]
                }),
            ),
            node(
                "3997:46299",
                "VECTOR",
                json!({"name": "Vector", "parentId": "3997:46298"}),
            ),
            node(
                "3997:46300",
                "VECTOR",
                json!({"name": "Vector", "parentId": "3997:46298"}),
            ),
            node(
                "3997:46301",
                "TEXT",
                json!({"name": "카카오로 공유하기", "parentId": "3997:46297"}),
            ),
        ],
    );

    assert_eq!(manifest.assets.len(), 1);
    assert_eq!(manifest.assets[0].asset_id, "3997:46298:node");
    assert_eq!(manifest.assets[0].node_id, "3997:46298");
    assert_eq!(manifest.assets[0].field, "node");
    assert_eq!(manifest.assets[0].source_kind, "vector-node");
    assert_eq!(manifest.assets[0].image_hash, None);
}

#[test]
fn bare_vector_is_an_svg_asset_but_text_is_not() {
    let manifest = manifest_for(
        &["1:vector", "1:text"],
        vec![
            node("1:vector", "VECTOR", json!({})),
            node("1:text", "TEXT", json!({})),
        ],
    );

    assert_eq!(manifest.assets.len(), 1);
    assert_eq!(manifest.assets[0].asset_id, "1:vector:node");
    assert_eq!(manifest.assets[0].source_kind, "vector-node");
}

#[test]
fn decorated_single_child_containers_do_not_replace_their_children() {
    let manifest = manifest_for(
        &["1:padding", "1:filled"],
        vec![
            node(
                "1:padding",
                "FRAME",
                json!({"childrenIds": ["1:padding-vector"], "paddingLeft": 8}),
            ),
            node(
                "1:padding-vector",
                "VECTOR",
                json!({"parentId": "1:padding"}),
            ),
            node(
                "1:filled",
                "FRAME",
                json!({
                    "childrenIds": ["1:filled-vector"],
                    "fills": [{"type": "SOLID", "visible": true}]
                }),
            ),
            node("1:filled-vector", "VECTOR", json!({"parentId": "1:filled"})),
        ],
    );

    let asset_ids = manifest
        .assets
        .iter()
        .map(|asset| asset.asset_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        asset_ids,
        vec!["1:filled-vector:node", "1:padding-vector:node"]
    );
}

#[test]
fn asset_leaf_with_one_non_tiled_image_fill_is_a_png_asset() {
    let manifest = manifest_for(
        &["1:image"],
        vec![node(
            "1:image",
            "RECTANGLE",
            json!({
                "isAsset": true,
                "fills": [{"type": "IMAGE", "scaleMode": "FILL", "imageRef": "image-ref-123"}]
            }),
        )],
    );

    assert_eq!(manifest.assets.len(), 1);
    assert_eq!(manifest.assets[0].asset_id, "1:image:fills:0");
    assert_eq!(manifest.assets[0].field, "fills/0");
    assert_eq!(manifest.assets[0].source_kind, "image-fill");
    assert_eq!(
        manifest.assets[0].image_hash.as_deref(),
        Some("image-ref-123")
    );
}

#[test]
fn exported_asset_validates_descriptor_bytes_hash_and_requested_settings() {
    let bytes = b"synthetic-png";
    let request = AssetRequest {
        asset_id: "1:1:fills:1".to_owned(),
        node_id: "1:1".to_owned(),
        field: "fills/1".to_owned(),
        image_hash: Some("image-hash-123".to_owned()),
        format: AssetFormat::Png,
        scale: 2,
    };
    let result = UpstreamResult {
        raw: json!({"content": [
            {"type":"text","text": json!({
                "kind":"devupAssetExport","fileKey":"FileKey123","version":"v1",
                "assetId":"1:1:fills:1","nodeId":"1:1","field":"fills/1",
                "imageHash":"image-hash-123","format":"png","scale":2,
                "status":"exported","byteLength":bytes.len(),
                "sha256":"294ad7145322ec19f8250cca8480a933f1ce8c9e2ad1038e7ae8930d55a6598a"
            }).to_string()},
            {"type":"image","data":STANDARD.encode(bytes),"mimeType":"image/png"}
        ]}),
    };

    let AssetExportOutcome::Entry(exported) =
        asset_export_from_result(&result, "FileKey123", Some("v1"), &request).unwrap()
    else {
        panic!("a PNG is exported in one answer")
    };
    assert_eq!(exported.status, AssetStatus::Exported);
    assert_eq!(exported.byte_length, Some(bytes.len()));
    let encoded = STANDARD.encode(bytes);
    assert_eq!(exported.data_base64.as_deref(), Some(encoded.as_str()));

    let mut mismatched = result;
    mismatched.raw["content"][0]["text"] = json!({
        "kind":"devupAssetExport","fileKey":"FileKey123","version":"v1",
        "assetId":"wrong","nodeId":"1:1","field":"fills/1","imageHash":"image-hash-123",
        "format":"png","scale":2,"status":"exported","byteLength":bytes.len(),
        "sha256":"294ad7145322ec19f8250cca8480a933f1ce8c9e2ad1038e7ae8930d55a6598a"
    })
    .to_string()
    .into();
    assert!(asset_export_from_result(&mismatched, "FileKey123", Some("v1"), &request).is_err());
}

/// An SVG past what one text answer holds is announced, not written: the
/// export script answers `chunked` with the length and hash, the collector
/// reads it back through the large-value script under the virtual field
/// `$export:svg` a fragment at a time, and the bytes the fragments assemble
/// to are the asset. Before this a 14 KB logo failed outright.
#[test]
fn an_svg_over_the_inline_cap_arrives_in_fragments() {
    use devup_mcp_figma::{LargeValueReadOptions, SVG_EXPORT_FIELD};

    let target =
        FigmaTarget::parse("https://www.figma.com/design/FileKey123/Fixture?node-id=1-1").unwrap();
    let mut request = CollectionRequest::new(target, CollectionScope::Node);
    request.asset_selections = vec![AssetSelection {
        asset_id: "1:1:node".to_owned(),
        format: AssetFormat::Svg,
        scale: 1,
    }];
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(metadata) = collector.advance().unwrap() else {
        panic!("metadata")
    };
    collector
        .accept(
            &metadata.id,
            UpstreamResult {
                raw: json!({"structuredContent":{"devupMetadata":{
                    "fileKey":"FileKey123","version":"v1","rootId":"1:1",
                    "nodes":[{"id":"1:1","type":"VECTOR","childrenIds":[],"descendantCount":0}]
                }}}),
            },
        )
        .unwrap();
    let CollectorStep::Call(snapshot_call) = collector.advance().unwrap() else {
        panic!("snapshot")
    };
    let vector = node(
        "1:1",
        "VECTOR",
        json!({"childrenIds": [], "isAsset": true, "fills": [{"type": "SOLID", "color": {"r": 0, "g": 0, "b": 0}, "visible": true}]}),
    );
    collector
        .accept(
            &snapshot_call.id,
            UpstreamResult {
                raw: json!({
                    "fileKey":"FileKey123","version":"v1","rootIds":["1:1"],
                    "nodes":[
                        serde_json::to_value(vector).unwrap(),
                        {"id":"__DEVUP_SNAPSHOT_CURSOR__","type":"DEVUP_INTERNAL","fields":{"offset":0,"nextOffset":1,"complete":true,"totalNodes":1},"extra":{},"fieldErrors":{}}
                    ],"diagnostics":[]
                }),
            },
        )
        .unwrap();

    // The export announces an SVG of 40 bytes, to come in two fragments.
    let svg = b"<svg xmlns='http://www.w3.org/2000/svg'/>";
    assert_eq!(svg.len(), 41);
    let sha256: String = sha2::Sha256::digest(svg)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let CollectorStep::Call(asset_call) = collector.advance().unwrap() else {
        panic!("explicit asset export")
    };
    collector
        .accept(
            &asset_call.id,
            UpstreamResult {
                raw: json!({"content": [{"type": "text", "text": json!({
                    "kind":"devupAssetExport","fileKey":"FileKey123","version":"v1",
                    "assetId":"1:1:node","nodeId":"1:1","field":"node",
                    "imageHash":null,"format":"svg","scale":1,
                    "status":"chunked","byteLength":svg.len(),"sha256":sha256,
                    "cursor":{"nextOffset":0,"maxChunkBytes":24},"errorCode":null
                }).to_string()}]}),
            },
        )
        .unwrap();

    // Then the fragments, each asked for under the virtual field.
    let mut offset = 0;
    while offset < svg.len() {
        let CollectorStep::Call(fragment_call) = collector.advance().unwrap() else {
            panic!("a fragment read is expected at offset {offset}")
        };
        let ReadToolCall::LargeValue { options, .. } = &fragment_call.call else {
            panic!("fragment reads go through the large-value script")
        };
        let LargeValueReadOptions {
            node_id,
            field,
            offset: asked,
            max_chunk_bytes,
            ..
        } = options;
        assert_eq!(node_id, "1:1");
        assert_eq!(field, SVG_EXPORT_FIELD);
        assert_eq!(*asked, offset);
        let next = (offset + max_chunk_bytes).min(svg.len());
        collector
            .accept(
                &fragment_call.id,
                UpstreamResult {
                    raw: json!({"content": [{"type": "text", "text": json!({
                        "kind":"devupLargeValueFragment","fileKey":"FileKey123","version":"v1",
                        "nodeId":"1:1","field":SVG_EXPORT_FIELD,
                        "offset":offset,"nextOffset":next,"byteLength":svg.len(),"sha256":sha256,
                        "dataBase64":STANDARD.encode(&svg[offset..next]),"complete":next == svg.len()
                    }).to_string()}]}),
                },
            )
            .unwrap();
        offset = next;
    }

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("complete")
    };
    assert_eq!(parts.assets.len(), 1);
    let exported = &parts.assets[0];
    assert_eq!(exported.status, AssetStatus::Exported);
    assert_eq!(exported.asset_id, "1:1:node");
    assert_eq!(exported.field, "node");
    assert_eq!(exported.byte_length, Some(svg.len()));
    assert_eq!(exported.mime_type.as_deref(), Some("image/svg+xml"));
    assert_eq!(
        STANDARD
            .decode(exported.data_base64.as_deref().unwrap())
            .unwrap(),
        svg
    );
    // The announcement, then two fragments of 24 bytes.
    assert_eq!(parts.stats.figma_tool_calls, 5);
}

/// A PNG past what one attachment carries comes back in fragments too.
/// Figma's remote MCP returns a written PNG as an attachment only up to
/// about a megabyte once base64-encoded: the devup-ui landing page's hero,
/// 950 KB at 1232x1232, was written, reported exported, and never arrived.
/// Now the export script announces it `chunked` and the collector reads it
/// back under `$export:png@<scale>`, the scale on the field so the re-export
/// behind each fragment is the same bytes that were announced.
#[test]
fn a_png_over_the_attachment_cap_arrives_in_fragments() {
    use devup_mcp_figma::{LargeValueReadOptions, PNG_EXPORT_FIELD};

    let target =
        FigmaTarget::parse("https://www.figma.com/design/FileKey123/Fixture?node-id=1-1").unwrap();
    let mut request = CollectionRequest::new(target, CollectionScope::Node);
    request.asset_selections = vec![AssetSelection {
        asset_id: "1:1:fills:0".to_owned(),
        format: AssetFormat::Png,
        scale: 2,
    }];
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(metadata) = collector.advance().unwrap() else {
        panic!("metadata")
    };
    collector
        .accept(
            &metadata.id,
            UpstreamResult {
                raw: json!({"structuredContent":{"devupMetadata":{
                    "fileKey":"FileKey123","version":"v1","rootId":"1:1",
                    "nodes":[{"id":"1:1","type":"RECTANGLE","childrenIds":[],"descendantCount":0}]
                }}}),
            },
        )
        .unwrap();
    let CollectorStep::Call(snapshot_call) = collector.advance().unwrap() else {
        panic!("snapshot")
    };
    let picture = node(
        "1:1",
        "RECTANGLE",
        json!({"childrenIds": [], "isAsset": true, "fills": [{"type": "IMAGE", "imageHash": "abc", "scaleMode": "FILL", "visible": true}]}),
    );
    collector
        .accept(
            &snapshot_call.id,
            UpstreamResult {
                raw: json!({
                    "fileKey":"FileKey123","version":"v1","rootIds":["1:1"],
                    "nodes":[
                        serde_json::to_value(picture).unwrap(),
                        {"id":"__DEVUP_SNAPSHOT_CURSOR__","type":"DEVUP_INTERNAL","fields":{"offset":0,"nextOffset":1,"complete":true,"totalNodes":1},"extra":{},"fieldErrors":{}}
                    ],"diagnostics":[]
                }),
            },
        )
        .unwrap();

    // The export announces a PNG of 100 bytes, to come in three fragments.
    // Binary, not text: every byte value below is outside UTF-8.
    let png: Vec<u8> = (0..100u8).map(|i| 0x80 | i).collect();
    let sha256: String = sha2::Sha256::digest(&png)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let CollectorStep::Call(asset_call) = collector.advance().unwrap() else {
        panic!("explicit asset export")
    };
    collector
        .accept(
            &asset_call.id,
            UpstreamResult {
                raw: json!({"content": [{"type": "text", "text": json!({
                    "kind":"devupAssetExport","fileKey":"FileKey123","version":"v1",
                    "assetId":"1:1:fills:0","nodeId":"1:1","field":"fills/0",
                    "imageHash":"abc","format":"png","scale":2,
                    "status":"chunked","byteLength":png.len(),"sha256":sha256,
                    "cursor":{"nextOffset":0,"maxChunkBytes":40},"errorCode":null
                }).to_string()}]}),
            },
        )
        .unwrap();

    let expected_field = format!("{PNG_EXPORT_FIELD}@2");
    let mut offset = 0;
    while offset < png.len() {
        let CollectorStep::Call(fragment_call) = collector.advance().unwrap() else {
            panic!("a fragment read is expected at offset {offset}")
        };
        let ReadToolCall::LargeValue { options, .. } = &fragment_call.call else {
            panic!("fragment reads go through the large-value script")
        };
        let LargeValueReadOptions {
            node_id,
            field,
            offset: asked,
            max_chunk_bytes,
            ..
        } = options;
        assert_eq!(node_id, "1:1");
        assert_eq!(field, &expected_field, "the scale rides on the field");
        assert_eq!(*asked, offset);
        let next = (offset + max_chunk_bytes).min(png.len());
        collector
            .accept(
                &fragment_call.id,
                UpstreamResult {
                    raw: json!({"content": [{"type": "text", "text": json!({
                        "kind":"devupLargeValueFragment","fileKey":"FileKey123","version":"v1",
                        "nodeId":"1:1","field":expected_field,
                        "offset":offset,"nextOffset":next,"byteLength":png.len(),"sha256":sha256,
                        "dataBase64":STANDARD.encode(&png[offset..next]),"complete":next == png.len()
                    }).to_string()}]}),
                },
            )
            .unwrap();
        offset = next;
    }

    let CollectorStep::Complete(parts) = collector.advance().unwrap() else {
        panic!("complete")
    };
    assert_eq!(parts.assets.len(), 1);
    let exported = &parts.assets[0];
    assert_eq!(exported.status, AssetStatus::Exported);
    assert_eq!(exported.asset_id, "1:1:fills:0");
    assert_eq!(exported.field, "fills/0");
    assert_eq!(exported.format, Some(AssetFormat::Png));
    assert_eq!(exported.scale, Some(2));
    assert_eq!(exported.byte_length, Some(png.len()));
    assert_eq!(exported.mime_type.as_deref(), Some("image/png"));
    assert_eq!(
        STANDARD
            .decode(exported.data_base64.as_deref().unwrap())
            .unwrap(),
        png
    );
    // The announcement, then three fragments of 40 bytes.
    assert_eq!(parts.stats.figma_tool_calls, 6);
}

/// A PDF is neither text nor an attachment Figma returns, so an announced
/// fragment transport for one is refused where the announcement is read,
/// not after fragments have been fetched for nothing.
#[test]
fn only_svg_and_png_exports_are_carried_in_fragments() {
    let request = AssetRequest {
        asset_id: "1:1:node".to_owned(),
        node_id: "1:1".to_owned(),
        field: "node".to_owned(),
        image_hash: None,
        format: AssetFormat::Pdf,
        scale: 1,
    };
    let result = json!({"content": [{"type": "text", "text": json!({
        "kind":"devupAssetExport","fileKey":"FileKey123","version":"v1",
        "assetId":"1:1:node","nodeId":"1:1","field":"node",
        "imageHash":null,"format":"pdf","scale":1,
        "status":"chunked","byteLength":100,"sha256":"00",
        "cursor":{"nextOffset":0,"maxChunkBytes":40},"errorCode":null
    }).to_string()}]});
    let error = asset_export_from_result(
        &UpstreamResult { raw: result },
        "FileKey123",
        Some("v1"),
        &request,
    )
    .unwrap_err();
    assert!(error.to_string().contains("SVG or PNG"), "{error}");
}

/// A frame that draws nothing of its own and holds a single picture is that
/// picture, and the code names the file after the frame. It does not carry
/// the fill, though, so it was listed as `<frame>:fills:0` - a fill index
/// the node does not have, which Figma can only answer with
/// DEVUP_ASSET_SOURCE_CHANGED. The devup-ui landing page's footer logo sits
/// in such a frame and was the one asset of 182 that never arrived. It is
/// listed as the node instead, which renders to the same picture.
#[test]
fn a_frame_holding_one_picture_is_exported_as_the_node_not_a_fill_it_lacks() {
    let manifest = manifest_for(
        &["1:frame"],
        vec![
            node(
                "1:frame",
                "FRAME",
                json!({"name": "Logo", "childrenIds": ["1:picture"]}),
            ),
            node(
                "1:picture",
                "FRAME",
                json!({
                    "name": "Bitmap",
                    "parentId": "1:frame",
                    "isAsset": true,
                    "childrenIds": [],
                    "fills": [{"type": "IMAGE", "scaleMode": "FIT", "imageHash": "image-hash-123"}]
                }),
            ),
        ],
    );

    assert_eq!(manifest.assets.len(), 1);
    let asset = &manifest.assets[0];
    assert_eq!(asset.asset_id, "1:frame:node");
    assert_eq!(asset.node_id, "1:frame", "the frame is what the code names");
    assert_eq!(
        asset.field, "node",
        "the frame has no fills, so its bytes come from rendering it"
    );
    assert_eq!(asset.source_kind, "image-node");
    assert_eq!(asset.image_hash.as_deref(), Some("image-hash-123"));
    assert_eq!(asset.status, AssetStatus::Available);

    // And the request that reaches Figma names the node, so the export
    // script's fill check - the one that refused this - never applies.
    let requests = devup_mcp_figma::resolve_asset_selections(
        &Snapshot {
            file_key: "FileKey123".to_owned(),
            version: Some("v1".to_owned()),
            roots: vec!["1:frame".to_owned()],
            nodes: [
                node(
                    "1:frame",
                    "FRAME",
                    json!({"name": "Logo", "childrenIds": ["1:picture"]}),
                ),
                node(
                    "1:picture",
                    "FRAME",
                    json!({
                        "name": "Bitmap",
                        "parentId": "1:frame",
                        "isAsset": true,
                        "childrenIds": [],
                        "fills": [{"type": "IMAGE", "scaleMode": "FIT", "imageHash": "image-hash-123"}]
                    }),
                ),
            ]
            .into_iter()
            .map(|node| (node.id.clone(), node))
            .collect(),
            diagnostics: Vec::new(),
        },
        &[AssetSelection {
            asset_id: "1:frame:node".to_owned(),
            format: AssetFormat::Png,
            scale: 1,
        }],
    )
    .expect("the frame is exportable");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].node_id, "1:frame");
    assert_eq!(requests[0].field, "node");
    assert_eq!(
        devup_mcp_figma::source_kind_of(&requests[0]),
        "image-node",
        "what the export reports has to match what discovery listed"
    );
}
