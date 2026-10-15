use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp_figma::{
    AssetStatus, CollectionRequest, CollectionScope, CollectorSession, CollectorStep, DevupError,
    ErrorCode, FigmaTarget, ReadToolCall, UpstreamResult,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
};

fn png() -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(8, 8)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}
fn hash(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn collector(preset: bool, lossless: bool) -> CollectorSession {
    let target =
        FigmaTarget::parse("https://www.figma.com/design/FileKey123/Fixture?node-id=1-2").unwrap();
    let mut request = CollectionRequest::new(target, CollectionScope::Node);
    request.web_ready_images = preset;
    request.resource_scope = devup_mcp_figma::ResourceScope::Used;
    if !preset {
        request.asset_selections = vec![devup_mcp_figma::AssetSelection {
            asset_id: "pic-0:fills:0".into(),
            format: devup_mcp_figma::AssetFormat::Webp,
            scale: 2,
            render_mode: devup_mcp_figma::AssetRenderMode::Node,
            quality: Some(43),
            lossless: Some(lossless),
        }];
    }
    let mut collector = CollectorSession::new(request);
    let CollectorStep::Call(first) = collector.advance().unwrap() else {
        panic!("snapshot")
    };
    let mut nodes = vec![
        json!({"id":"1:2","type":"FRAME","fields":{"name":"Root","width":100,"height":100,
  "childrenIds":(0..15).map(|i|format!("pic-{i}")).chain(["boundary".into()]).collect::<Vec<_>>()},"extra":{},"fieldErrors":{}}),
    ];
    for i in 0..15 {
        nodes.push(json!({"id":format!("pic-{i}"),"type":"RECTANGLE","fields":{"name":"Picture","visible":i!=14,"width":8,"height":8,
  "fills":if i==0 {json!([{"type":"IMAGE","imageHash":"source","scaleMode":"CROP"},{"type":"IMAGE","imageHash":"second","scaleMode":"FILL"}])}
    else if i==1 {json!([{"type":"IMAGE","imageHash":"source","scaleMode":"FILL"},{"type":"IMAGE","imageHash":"hidden","visible":false},{"type":"IMAGE","imageHash":"transparent","opacity":0}])}
    else {json!([{"type":"IMAGE","imageHash":"source","scaleMode":"FILL"}])},"effects":[]},"extra":{},"fieldErrors":{}}));
    }
    nodes.push(json!({"id":"boundary","type":"FRAME","fields":{"name":"Boundary","childrenIds":["nested"],"width":8,"height":8},"extra":{},"fieldErrors":{}}));
    nodes.push(json!({"id":"nested","type":"RECTANGLE","fields":{"name":"Nested","isAsset":true,"width":8,"height":8,"fills":[{"type":"IMAGE","imageHash":"nested","scaleMode":"FILL"}],"effects":[]},"extra":{},"fieldErrors":{}}));
    let mut envelope = json!({"kind":"devupFastSnapshotEnvelope","schemaVersion":1,"source":{"fileKey":"FileKey123","rootId":"1:2"},
  "snapshot":{"fileKey":"FileKey123","version":"v1","rootIds":["1:2"],"nodes":nodes,"diagnostics":[]},
  "resources":{"collections":[],"variables":[],"styles":[],"usedRemoteVariables":[],"localComplete":false,"usedRemoteComplete":true,"unresolved":[]},
  "integrity":{"nodeCount":nodes.len(),"variableRefCount":0,"styleRefCount":0,"utf8Bytes":0}});
    loop {
        let len = envelope.to_string().len();
        if envelope["integrity"]["utf8Bytes"] == len {
            break;
        }
        envelope["integrity"]["utf8Bytes"] = json!(len);
    }
    let result = UpstreamResult {
        raw: json!({"content":[{"type":"text","text":envelope.to_string()}]}),
    };
    devup_mcp_figma::decode_fast_snapshot(
        &result,
        &FigmaTarget::parse("https://www.figma.com/design/FileKey123/Fixture?node-id=1-2").unwrap(),
    )
    .unwrap();
    collector.accept(&first.id, result).unwrap();
    collector
}
fn export_result(request: &devup_mcp_figma::AssetRequest, chunked: bool) -> UpstreamResult {
    let bytes = png();
    let mut value = json!({"kind":"devupAssetExport","fileKey":"FileKey123","version":"v1","assetId":request.asset_id,
  "nodeId":request.node_id,"field":request.field,"imageHash":request.image_hash,"format":request.format,"scale":request.scale,"renderMode":request.render_mode,
  "status":if chunked {"chunked"}else{"exported"},"byteLength":bytes.len(),"sha256":hash(&bytes),
  "exportDiagnostics":{"method":"native","stages":[],"renderInfo":{"representation":"native-node","nodeSize":{"width":8,"height":8,"aspectRatio":1},"effects":[]}}});
    if chunked {
        value["cursor"] = json!({"nextOffset":0,"maxChunkBytes":12000});
    } else {
        value["mimeType"] = json!("image/png");
        value["data"] = json!(STANDARD.encode(&bytes));
    }
    UpstreamResult { raw: value }
}
#[test]
fn preset_schedules_bounded_batches_and_retains_nested_hidden_and_multifill_rows() {
    let mut collector = collector(true, false);
    let mut issued = BTreeSet::new();
    let mut chunk_nodes = BTreeMap::new();
    let mut first_batch = Vec::new();
    // The collector can fill its pending window, but cannot schedule asset seven
    // while any member of its first six-asset batch remains outstanding.
    while let CollectorStep::Call(call) = collector.advance().unwrap() {
        first_batch.push(call);
    }
    assert!(first_batch.len() <= 6);
    let held = first_batch.pop().unwrap();
    let mut calls_in_batch = first_batch.len() + 1;
    for call in first_batch {
        let ReadToolCall::AssetExport { request, .. } = &call.call else {
            panic!("asset")
        };
        issued.insert(request.asset_id.clone());
        collector
            .accept(&call.id, export_result(request, false))
            .unwrap();
    }
    while let CollectorStep::Call(call) = collector.advance().unwrap() {
        calls_in_batch += 1;
        let ReadToolCall::AssetExport { request, .. } = &call.call else {
            panic!("asset")
        };
        issued.insert(request.asset_id.clone());
        collector
            .accept(&call.id, export_result(request, false))
            .unwrap();
    }
    assert_eq!(calls_in_batch, 6);
    let ReadToolCall::AssetExport { request, .. } = &held.call else {
        panic!("asset")
    };
    issued.insert(request.asset_id.clone());
    collector
        .reject(
            &held.id,
            &DevupError::new(
                ErrorCode::DevupInvalidInput,
                "synthetic failed native read",
                false,
            ),
        )
        .unwrap();
    let parts = loop {
        match collector.advance().unwrap() {
            CollectorStep::Call(call) => match &call.call {
                ReadToolCall::AssetExport { request, .. } => {
                    assert!(issued.insert(request.asset_id.clone()));
                    assert!(
                        chunk_nodes
                            .insert(request.node_id.clone(), request.asset_id.clone())
                            .is_none(),
                        "same-node PNG fragment must not collide"
                    );
                    collector
                        .accept(&call.id, export_result(request, true))
                        .unwrap();
                }
                ReadToolCall::LargeValue { options, .. } => {
                    let bytes = png();
                    chunk_nodes.remove(&options.node_id);
                    collector.accept(&call.id,UpstreamResult{raw:json!({"fileKey":"FileKey123","version":"v1","nodeId":options.node_id,
    "field":options.field,"offset":0,"nextOffset":bytes.len(),"byteLength":bytes.len(),"sha256":hash(&bytes),"dataBase64":STANDARD.encode(&bytes),"complete":true})}).unwrap();
                }
                other => panic!("unexpected {other:?}"),
            },
            CollectorStep::Complete(parts) => break parts,
            CollectorStep::AwaitingResults => panic!("no unhandled pending calls"),
        }
    };
    assert!(
        parts.assets.len() >= 20,
        "19 census paints plus code raster boundary: {:?}",
        parts.assets.iter().map(|a| &a.asset_id).collect::<Vec<_>>()
    );
    assert!(parts.assets.iter().any(|a| a.asset_id == "nested:fills:0"));
    assert!(parts.assets.iter().any(|a| a.asset_id == "boundary:node"));
    assert!(parts.assets.iter().any(|a| a.asset_id == "pic-0:fills:1"));
    assert_eq!(
        parts
            .assets
            .iter()
            .filter(|a| a.status == AssetStatus::Failed)
            .count(),
        4
    );
    for id in ["pic-1:fills:1", "pic-1:fills:2", "pic-14:fills:0"] {
        assert!(
            parts
                .assets
                .iter()
                .any(|a| a.asset_id == id && a.status == AssetStatus::Failed)
        );
    }
    for asset in parts.assets {
        assert_eq!(asset.quality, Some(80));
        assert_eq!(asset.lossless, Some(false));
        if asset.status == AssetStatus::Exported {
            let info = asset.render_info.unwrap();
            assert_eq!(info["representation"], "native-node-transcoded");
            assert_eq!(info["fidelity"], "approximate");
            assert_eq!(info["byteEvidence"]["sha256"], asset.sha256.unwrap());
            assert!(info["compression"]["referencePNG"].is_object());
        }
    }
}

#[test]
fn malformed_fragmented_native_png_fails_one_asset_and_keeps_the_entire_preset() {
    let mut c = collector(true, false);
    let mut bad = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(
        2,
        2,
        image::Rgba([1u16, 2, 3, 4]),
    ))
    .write_to(&mut bad, image::ImageFormat::Png)
    .unwrap();
    let bad = bad.into_inner();
    let mut failed_id = None;
    let parts = loop {
        match c.advance().unwrap() {
            CollectorStep::Call(call) => match &call.call {
                ReadToolCall::AssetExport { request, .. } => {
                    if failed_id.is_none() {
                        failed_id = Some(request.asset_id.clone());
                        let mut result = export_result(request, true);
                        result.raw["byteLength"] = json!(bad.len());
                        result.raw["sha256"] = json!(hash(&bad));
                        c.accept(&call.id, result).unwrap();
                    } else {
                        c.accept(&call.id, export_result(request, false)).unwrap();
                    }
                }
                ReadToolCall::LargeValue { options, .. } => {
                    c.accept(&call.id,UpstreamResult{raw:json!({"fileKey":"FileKey123","version":"v1","nodeId":options.node_id,"field":options.field,"offset":0,
    "nextOffset":bad.len(),"byteLength":bad.len(),"sha256":hash(&bad),"dataBase64":STANDARD.encode(&bad),"complete":true})}).unwrap();
                }
                other => panic!("unexpected {other:?}"),
            },
            CollectorStep::Complete(parts) => break parts,
            CollectorStep::AwaitingResults => panic!("unexpected wait"),
        }
    };
    assert_eq!(parts.assets.len(), 20);
    let failed = parts
        .assets
        .iter()
        .find(|a| Some(&a.asset_id) == failed_id.as_ref())
        .unwrap();
    assert_eq!(failed.status, AssetStatus::Failed);
    assert_eq!(
        failed.error_code.as_deref(),
        Some("DEVUP_ASSET_WEBP_POSTPROCESS_FAILED")
    );
    assert_eq!(failed.quality, Some(80));
    assert_eq!(failed.lossless, Some(false));
    assert!(failed.data_base64.is_none());
    assert!(
        parts
            .assets
            .iter()
            .any(|a| a.status == AssetStatus::Exported)
    );
}
#[test]
fn chunked_webp_has_final_byte_evidence_and_requested_identity_for_both_modes() {
    for lossless in [false, true] {
        let mut c = collector(false, lossless);
        let CollectorStep::Call(call) = c.advance().unwrap() else {
            panic!("asset")
        };
        let ReadToolCall::AssetExport { request, .. } = &call.call else {
            panic!("asset")
        };
        c.accept(&call.id, export_result(request, true)).unwrap();
        let CollectorStep::Call(call) = c.advance().unwrap() else {
            panic!("fragment")
        };
        let ReadToolCall::LargeValue { options, .. } = &call.call else {
            panic!("fragment")
        };
        let bytes = png();
        c.accept(&call.id,UpstreamResult{raw:json!({"fileKey":"FileKey123","version":"v1","nodeId":options.node_id,"field":options.field,"offset":0,
   "nextOffset":bytes.len(),"byteLength":bytes.len(),"sha256":hash(&bytes),"dataBase64":STANDARD.encode(&bytes),"complete":true})}).unwrap();
        let CollectorStep::Complete(parts) = c.advance().unwrap() else {
            panic!("complete")
        };
        let asset = &parts.assets[0];
        let info = asset.render_info.as_ref().unwrap();
        assert_eq!(asset.quality, Some(43));
        assert_eq!(asset.lossless, Some(lossless));
        assert_eq!(
            info["fidelity"],
            if lossless {
                "lossless-pixels"
            } else {
                "approximate"
            }
        );
        assert_eq!(
            info["byteEvidence"]["sha256"],
            asset.sha256.as_deref().unwrap()
        );
        assert_eq!(info["compression"]["referencePNG"]["sha256"], hash(&bytes));
        assert_ne!(info["byteEvidence"]["sha256"], hash(&bytes));
        assert_eq!(info["representation"], "native-node-transcoded");
    }
}
