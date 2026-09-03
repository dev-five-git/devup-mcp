use std::collections::BTreeMap;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp_figma::{
    AssetFormat, AssetRequest, AssetSelection, AssetStatus, CollectionRequest, CollectionScope,
    CollectorSession, CollectorStep, FigmaTarget, RawNode, ReadToolCall, Snapshot, UpstreamResult,
    asset_export_from_result, discover_asset_manifest,
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
        asset_id: "1:1:fills:1".to_owned(),
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
    assert_eq!(request.asset_id, "1:1:fills:1");
    collector
        .accept(
            &asset_call.id,
            UpstreamResult {
                raw: json!({
                    "kind":"devupAssetExport","fileKey":"FileKey123","version":"v1",
                    "assetId":"1:1:fills:1","nodeId":"1:1","field":"fills/1",
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

    let entry = asset_export_from_result(&result, "FileKey123", Some("v1"), &request)
        .expect("an inlined SVG payload must decode");

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
        nodes: [
            node(
                "1:1",
                "FRAME",
                json!({
                    "childrenIds": ["1:2"],
                    "fills": [
                        {"type": "SOLID", "color": {"r": 1, "g": 1, "b": 1}},
                        {"type": "IMAGE", "imageHash": "image-hash-123", "scaleMode": "FILL"}
                    ]
                }),
            ),
            node(
                "1:2",
                "VECTOR",
                json!({"parentId": "1:1", "childrenIds": [], "fills": []}),
            ),
        ]
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
    assert_eq!(manifest.assets.len(), 2);
    assert_eq!(manifest.assets[0].asset_id, "1:1:fills:1");
    assert_eq!(manifest.assets[0].node_id, "1:1");
    assert_eq!(manifest.assets[0].field, "fills/1");
    assert_eq!(manifest.assets[0].source_kind, "image-fill");
    assert_eq!(
        manifest.assets[0].image_hash.as_deref(),
        Some("image-hash-123")
    );
    assert_eq!(manifest.assets[0].status, AssetStatus::Available);
    assert_eq!(manifest.assets[1].asset_id, "1:2:node");
    assert_eq!(manifest.assets[1].source_kind, "vector-node");
    assert!(manifest.assets[1].data_base64.is_none());
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

    let exported = asset_export_from_result(&result, "FileKey123", Some("v1"), &request).unwrap();
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
