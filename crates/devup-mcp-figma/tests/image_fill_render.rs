use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    process::{Command, Stdio},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp_figma::{
    AssetExportOutcome, AssetFormat, AssetRenderMode, AssetRequest, AssetSelection, RawNode,
    ReadToolCall, Snapshot, UpstreamResult, annotate_assets, asset_export_from_result,
    discover_asset_manifest, exported_asset_from_bytes, resolve_asset_selections,
};
use serde_json::{Value, json};

fn request(mode: AssetRenderMode) -> AssetRequest {
    AssetRequest {
        usage_aspect_ratio: None,
        max_width: None,
        srcset: false,
        visible_scope_node_id: None,
        asset_id: "sample:fills:0".into(),
        node_id: "sample".into(),
        field: "fills/0".into(),
        image_hash: Some("source".into()),
        format: AssetFormat::Png,
        scale: 1.0,
        render_mode: mode,
        quality: None,
        lossless: None,
    }
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(w, h)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

fn execute(request: &AssetRequest) -> Value {
    let call = ReadToolCall::asset_export("fixture", None, request.clone());
    assert_eq!(
        call.bridge_job().unwrap().params["asset"]["renderMode"],
        json!(request.render_mode)
    );
    let code = call.arguments()["code"]
        .as_str()
        .unwrap()
        .replace("\"assetId\":", "\"transport\":\"bridge\",\"assetId\":");
    let input =
        json!({"code":code,"raw":STANDARD.encode(png(20,10)),"rendered":STANDARD.encode(png(4,4))});
    let script = r#"
const input=JSON.parse(require('node:fs').readFileSync(0,'utf8'));
let exports=0,reads=0,settings;
const node={id:'sample',type:'RECTANGLE',width:4,height:4,isMask:true,effects:[{type:'LAYER_BLUR',radius:3,visible:true}],
 fills:[{type:'IMAGE',imageHash:'source',scaleMode:'CROP',imageTransform:[[0.5,0,0.25],[0,1,0]]}],
 exportAsync:async s=>{exports++;settings=s;return new Uint8Array(Buffer.from(input.rendered,'base64'));}};
const figma={fileKey:'fixture',getNodeByIdAsync:async()=>node,base64Encode:b=>Buffer.from(b).toString('base64'),
 getImageByHash:()=>({getBytesAsync:async()=>{reads++;return new Uint8Array(Buffer.from(input.raw,'base64'));}})};
new (Object.getPrototypeOf(async function(){}).constructor)('figma',input.code)(figma)
 .then(data=>process.stdout.write(JSON.stringify({data,exports,reads,settings}))).catch(e=>{console.error(e);process.exitCode=1;});
"#;
    let mut child = Command::new("node")
        .args(["-e", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn decode(request: &AssetRequest, value: &Value) -> devup_mcp_figma::AssetManifestEntry {
    let AssetExportOutcome::Entry(entry) = asset_export_from_result(
        &UpstreamResult { raw: value.clone() },
        "fixture",
        None,
        request,
    )
    .unwrap() else {
        panic!("entry")
    };
    *entry
}

#[test]
fn compiled_raw_and_native_paths_measure_actual_bytes_and_diagnose_ratios() {
    for mode in [AssetRenderMode::Node, AssetRenderMode::Fill] {
        let request = request(mode);
        let result = execute(&request);
        assert_eq!(
            result["exports"],
            json!(usize::from(mode == AssetRenderMode::Node))
        );
        assert_eq!(
            result["reads"],
            json!(usize::from(mode == AssetRenderMode::Fill))
        );
        let entry = decode(&request, &result);
        assert_eq!(entry.render_mode, mode);
        let info = entry.render_info.unwrap();
        assert_eq!(
            info["outputSize"]["width"],
            if mode == AssetRenderMode::Node {
                4.0
            } else {
                20.0
            }
        );
        assert_eq!(
            info["outputSize"]["height"],
            if mode == AssetRenderMode::Node {
                4.0
            } else {
                10.0
            }
        );
        assert_eq!(
            info["aspectRatioComparison"],
            if mode == AssetRenderMode::Node {
                "match"
            } else {
                "mismatch"
            }
        );
        assert_eq!(info["dimensionEvidence"], "encoded-byte-header");
        assert_eq!(info["sourceProperties"]["scaleMode"], "CROP");
        assert_eq!(
            info["sourceProperties"]["blurEffects"][0]["type"],
            "LAYER_BLUR"
        );
        assert_eq!(info["sourceProperties"]["isMask"], true);
        assert_eq!(info["fidelity"], "exact");
        assert_eq!(info["canvasFidelity"], "unverified");
        assert_eq!(
            info["fidelityScope"],
            if mode == AssetRenderMode::Node {
                "native-node-export-at-requested-settings"
            } else {
                "original-source-bytes"
            }
        );
        assert_eq!(info["effectsBakedIntoAsset"], mode == AssetRenderMode::Node);
        assert_eq!(info["visibleSourceRatio"], Value::Null);
        assert_eq!(
            STANDARD.decode(entry.data_base64.unwrap()).unwrap(),
            if mode == AssetRenderMode::Node {
                png(4, 4)
            } else {
                png(20, 10)
            }
        );
    }
}

#[test]
fn compiled_node_and_explicit_fill_webp_finish_with_real_codec_and_truthful_scope() {
    for mode in [
        AssetRenderMode::Node,
        AssetRenderMode::Fill,
        AssetRenderMode::Auto,
    ] {
        let mut req = request(mode);
        req.format = AssetFormat::Webp;
        req.quality = Some(37);
        req.lossless = Some(false);
        let result = execute(&req);
        let entry = decode(&req, &result);
        let bytes = STANDARD.decode(entry.data_base64.unwrap()).unwrap();
        assert_eq!(
            image::guess_format(&bytes).unwrap(),
            image::ImageFormat::WebP
        );
        assert_eq!(entry.mime_type.as_deref(), Some("image/webp"));
        assert_eq!(entry.quality, Some(37));
        assert_eq!(entry.lossless, Some(false));
        let info = entry.render_info.unwrap();
        assert_eq!(info["fidelity"], "approximate");
        assert_ne!(info["fidelityScope"], "original-source-bytes");
        assert_eq!(
            info["renderedAs"],
            if mode == AssetRenderMode::Fill {
                "transcoded-fill"
            } else {
                "transcoded-node-render"
            }
        );
        assert_eq!(
            info["outputSize"]["width"],
            if mode == AssetRenderMode::Fill {
                20.0
            } else {
                4.0
            }
        );
        if mode != AssetRenderMode::Fill {
            assert_eq!(result["settings"]["format"], "PNG");
        }
    }
    let mut req = request(AssetRenderMode::Fill);
    req.format = AssetFormat::Webp;
    let mut result = execute(&req);
    result["data"]["status"] = json!("chunked");
    result["data"]["cursor"] = json!({"nextOffset":0,"maxChunkBytes":12000});
    assert!(
        asset_export_from_result(&UpstreamResult { raw: result }, "fixture", None, &req).is_err(),
        "fill WebP must not substitute native PNG fragments"
    );
    let mut automatic = request(AssetRenderMode::Auto);
    automatic.format = AssetFormat::Webp;
    let mut original = execute(&request(AssetRenderMode::Fill));
    original["data"]["renderMode"] = json!("auto");
    original["data"]["format"] = json!("webp");
    original["data"]["exportDiagnostics"]["renderInfo"]["nodeSize"] =
        json!({"width":20,"height":10,"aspectRatio":2});
    original["data"]["exportDiagnostics"]["renderInfo"]["autoDecision"] =
        json!({"eligible":true,"selected":"fill","reasons":[]});
    let error = asset_export_from_result(
        &UpstreamResult { raw: original },
        "fixture",
        None,
        &automatic,
    )
    .unwrap_err();
    assert!(error.message.contains("explicit renderMode fill"));
}

#[test]
fn receiver_rejects_cross_mode_descriptors_and_fill_node_fragment_substitution() {
    let request = request(AssetRenderMode::Fill);
    let mut result = execute(&request);
    result["data"]["renderMode"] = json!("node");
    assert!(
        asset_export_from_result(
            &UpstreamResult {
                raw: result.clone()
            },
            "fixture",
            None,
            &request
        )
        .is_err()
    );
    result["data"].as_object_mut().unwrap().remove("renderMode");
    assert!(
        asset_export_from_result(
            &UpstreamResult {
                raw: result.clone()
            },
            "fixture",
            None,
            &request
        )
        .is_err()
    );
    result["data"]["renderMode"] = json!("fill");
    result["data"]["status"] = json!("chunked");
    result["data"]["cursor"] = json!({"nextOffset":0,"maxChunkBytes":12000});
    result["data"]
        .as_object_mut()
        .unwrap()
        .remove("exportDiagnostics");
    assert!(
        asset_export_from_result(&UpstreamResult { raw: result }, "fixture", None, &request)
            .is_err()
    );
}

#[test]
fn absent_dimension_evidence_is_unknown_and_document_scale_is_not_claimed() {
    let mut request = request(AssetRenderMode::Node);
    let entry = exported_asset_from_bytes(&request, b"unrecognized-image").unwrap();
    let info = entry.render_info.unwrap();
    assert!(info["outputSize"].is_null());
    assert_eq!(info["aspectRatioComparison"], "unknown");
    assert_eq!(info["representation"], "unknown");
    request.format = AssetFormat::Svg;
    request.scale = 3.0;
    let info = exported_asset_from_bytes(&request, b"<svg/>")
        .unwrap()
        .render_info
        .unwrap();
    assert_eq!(info["requestedScale"], 3);
    assert_eq!(info["scaleSemantics"], "not-applied-to-vector-document");
}

fn snapshot() -> Snapshot {
    let node = |id: &str, kind: &str, fields: Value| RawNode {
        id: id.into(),
        node_type: kind.into(),
        fields: serde_json::from_value(fields).unwrap(),
        extra: Default::default(),
        field_errors: BTreeMap::new(),
    };
    let nodes = [
        node(
            "parent",
            "FRAME",
            json!({"name":"Section / ../CON:","childrenIds":["sample","other"]}),
        ),
        node(
            "sample",
            "RECTANGLE",
            json!({"name":"Rectangle 32","parentId":"parent","isAsset":true,"width":4,"height":4,"effects":[{"type":"LAYER_BLUR"}],"fills":[{"type":"IMAGE","imageHash":"source","scaleMode":"CROP","imageTransform":[[1,0,0],[0,1,0]]}]}),
        ),
        node(
            "other",
            "RECTANGLE",
            json!({"name":"Rectangle 32","parentId":"parent","isAsset":true,"fills":[{"type":"IMAGE","imageHash":"other","scaleMode":"FILL"}]}),
        ),
    ];
    Snapshot {
        file_key: "fixture".into(),
        version: None,
        roots: vec!["parent".into()],
        nodes: nodes.into_iter().map(|n| (n.id.clone(), n)).collect(),
        diagnostics: vec![],
    }
}

#[test]
fn duplicate_names_have_safe_stable_hierarchy_suggestions_and_enriched_measurements() {
    let snapshot = snapshot();
    let manifest = discover_asset_manifest(&snapshot);
    assert_eq!(manifest.assets.len(), 2);
    let names = manifest
        .assets
        .iter()
        .map(|a| a.suggested_filename.clone().unwrap())
        .collect::<Vec<_>>();
    assert_ne!(names[0], names[1]);
    for name in &names {
        assert!(name.starts_with("section-con-rectangle-32-"));
        assert!(
            name.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.'))
        );
        assert!(name.len() < 160);
        assert!(!name.contains(".."));
    }
    assert_eq!(
        names,
        discover_asset_manifest(&snapshot)
            .assets
            .iter()
            .map(|a| a.suggested_filename.clone().unwrap())
            .collect::<Vec<_>>()
    );
    let mut assets =
        vec![exported_asset_from_bytes(&request(AssetRenderMode::Node), &png(20, 10)).unwrap()];
    annotate_assets(&snapshot, &mut assets);
    assert_eq!(
        assets[0].render_info.as_ref().unwrap()["aspectRatioComparison"],
        "mismatch"
    );
    assert_eq!(
        assets[0].render_info.as_ref().unwrap()["sourceProperties"]["effects"][0]["type"],
        "LAYER_BLUR"
    );
}

#[test]
fn selection_defaults_to_original_source_and_refuses_unsupported_fill_settings() {
    let default: AssetSelection =
        serde_json::from_value(json!({"assetId":"sample:fills:0","format":"png","scale":1}))
            .unwrap();
    assert_eq!(default.render_mode, AssetRenderMode::OriginalSource);
    let fill = AssetSelection {
        usage_aspect_ratio: None,
        max_width: None,
        srcset: false,
        visible_scope_node_id: None,
        render_mode: AssetRenderMode::Fill,
        quality: None,
        lossless: None,
        ..default
    };
    assert_eq!(
        resolve_asset_selections(&snapshot(), std::slice::from_ref(&fill)).unwrap()[0].render_mode,
        AssetRenderMode::Fill
    );
    for selection in [
        AssetSelection {
            scale: 2.0,
            ..fill.clone()
        },
        AssetSelection {
            format: AssetFormat::Svg,
            ..fill.clone()
        },
        AssetSelection {
            format: AssetFormat::Pdf,
            ..fill.clone()
        },
    ] {
        assert!(resolve_asset_selections(&snapshot(), &[selection]).is_err());
    }
}

#[test]
fn unicode_parent_names_remain_readable_and_cannot_introduce_write_paths() {
    let mut snapshot = snapshot();
    snapshot
        .nodes
        .get_mut("parent")
        .unwrap()
        .fields
        .insert("name".into(), json!("예시 화면 / ..\\ 안전"));
    let assets = discover_asset_manifest(&snapshot).assets;
    for asset in assets {
        let name = asset.suggested_filename.unwrap();
        assert!(name.starts_with("예시-화면-안전-rectangle-32-"));
        assert!(!name.contains(['/', '\\', ':']));
        assert!(!name.contains(".."));
        assert!(name.len() < 160);
    }
}

#[test]
fn source_and_generated_plugin_image_fill_contracts() {
    let output = Command::new("node")
        .arg("--test")
        .arg(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/image_fill_render_script.mjs"),
        )
        .output()
        .expect("Node is required for the source/generated plugin contract");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn direct_leaf_filename_uses_runtime_parent_names_when_snapshot_has_no_ancestors() {
    let mut snapshot = snapshot();
    snapshot.nodes.retain(|id, _| id == "sample");
    snapshot.roots = vec!["sample".into()];
    let mut asset = exported_asset_from_bytes(&request(AssetRenderMode::Node), &png(4, 4)).unwrap();
    asset.render_info.as_mut().unwrap()["layerHierarchy"] =
        json!(["예시 화면", "Gallery / ..\\CON:", "Rectangle 32"]);
    let mut assets = vec![asset];
    annotate_assets(&snapshot, &mut assets);
    let name = assets[0].suggested_filename.as_ref().unwrap();
    assert!(
        name.starts_with("예시-화면-gallery-con-rectangle-32-"),
        "{name}"
    );
    assert!(!name.contains(['/', '\\', ':']));
    assert!(!name.contains(".."));
    assert!(name.len() < 160);
}

#[test]
fn auto_risky_native_capture_retains_scoped_evidence_and_cache_roundtrip() {
    let request = request(AssetRenderMode::Auto);
    let result = execute(&request);
    assert_eq!(result["exports"], 1);
    let mut entry = decode(&request, &result);
    let info = entry.render_info.as_ref().unwrap();
    assert_eq!(info["renderedAs"], "node-render");
    assert_eq!(info["autoDecision"]["eligible"], false);
    assert_eq!(info["effectsBakedIntoAsset"], true);
    assert!(
        info["warnings"]
            .as_array()
            .unwrap()
            .contains(&json!("blur-baked-in"))
    );
    assert_eq!(info["instanceOverride"], "unknown");
    let cached: devup_mcp_figma::AssetManifestEntry = serde_json::from_value(json!(entry)).unwrap();
    assert_eq!(entry, cached);
    annotate_assets(&snapshot(), std::slice::from_mut(&mut entry));
    assert_eq!(
        entry.render_info.as_ref().unwrap()["autoDecision"],
        cached.render_info.as_ref().unwrap()["autoDecision"]
    );
    assert_eq!(
        entry.render_info,
        entry.export_diagnostics.as_ref().unwrap().render_info
    );
    entry.status = devup_mcp_figma::AssetStatus::Failed;
    annotate_assets(&snapshot(), std::slice::from_mut(&mut entry));
    let info = entry.render_info.as_ref().unwrap();
    assert_eq!(info["renderedAs"], "not-exported");
    assert_eq!(info["fidelity"], "unverified");
    assert_eq!(info["effectsBakedIntoAsset"], false);
    assert!(info["outputSize"].is_null());
    assert_eq!(
        entry.render_info,
        entry.export_diagnostics.as_ref().unwrap().render_info
    );
}

#[test]
fn audit_counts_nodes_and_every_fill_even_below_flattened_assets() {
    let mut snapshot = snapshot();
    let node = snapshot.nodes.get_mut("sample").unwrap();
    node.fields.insert("fills".into(), json!([
        {"type":"IMAGE","imageHash":"one","scaleMode":"FILL","imageTransform":[[1,0,0],[0,1,0]]},
        {"type":"IMAGE","imageHash":"two","scaleMode":"FIT","imageTransform":[[0.3,0,0],[0,0.6,0]]},
        {"type":"GRADIENT_LINEAR","blendMode":"MULTIPLY"}
    ]));
    let audit = devup_mcp_figma::image_fill_audit(&snapshot, &[]);
    assert_eq!(audit["imageFillNodes"], 2);
    assert_eq!(audit["imageFills"], 3);
    assert_eq!(audit["total"], 3);
    assert_eq!(audit["exact"], 0);
    assert_eq!(audit["unknown"], 3);
    assert_eq!(audit["needsReview"], 3);
    assert_eq!(audit["reviewReasonCounts"]["multiple-fills"], 2);
    assert_eq!(audit["reviewReasonCounts"]["image-transform"], 1);
    assert!(
        audit["assets"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["canvasFidelity"] == "unverified")
    );
}

#[test]
fn auto_raw_receiver_requires_dimension_and_decision_evidence() {
    let mut result = execute(&request(AssetRenderMode::Fill));
    result["data"]["renderMode"] = json!("auto");
    let req = request(AssetRenderMode::Auto);
    assert!(
        asset_export_from_result(
            &UpstreamResult {
                raw: result.clone()
            },
            "fixture",
            None,
            &req
        )
        .is_err()
    );
    let info = &mut result["data"]["exportDiagnostics"]["renderInfo"];
    info["autoDecision"] = json!({"eligible":true,"selected":"fill","reasons":[]});
    // Source is 20x10; the node is 4x4. A claimed decision cannot override bytes.
    assert!(
        asset_export_from_result(
            &UpstreamResult {
                raw: result.clone()
            },
            "fixture",
            None,
            &req
        )
        .is_err()
    );
    result["data"]["exportDiagnostics"]["renderInfo"]["nodeSize"] =
        json!({"width":20,"height":10,"aspectRatio":2});
    let entry = decode(&req, &result);
    assert_eq!(
        entry.render_info.unwrap()["scaleSemantics"],
        "intrinsic-original-bytes-no-resize"
    );
}

#[test]
fn original_source_never_accepts_missing_mode_native_provenance_or_node_fragments() {
    for format in [AssetFormat::Png, AssetFormat::Jpg, AssetFormat::Webp] {
        let mut req = request(AssetRenderMode::OriginalSource);
        req.scale = 1.5;
        req.format = format;
        let valid = execute(&req);
        for mutation in 0..6 {
            let mut raw = valid.clone();
            match mutation {
                0 => {
                    raw["data"].as_object_mut().unwrap().remove("renderMode");
                }
                1 => {
                    raw["data"]["exportDiagnostics"]["renderInfo"]["representation"] =
                        json!("native-node");
                }
                2 => {
                    raw["data"]
                        .as_object_mut()
                        .unwrap()
                        .remove("exportDiagnostics");
                }
                3 => {
                    raw["data"]["exportDiagnostics"]["method"] = json!("native");
                }
                4 => {
                    raw["data"]["status"] = json!("chunked");
                    raw["data"]["cursor"] = json!({"nextOffset":0,"maxChunkBytes":12000});
                }
                _ => {
                    raw["data"]["maxWidth"] = json!(120);
                }
            }
            assert!(
                asset_export_from_result(&UpstreamResult { raw }, "fixture", None, &req).is_err(),
                "mutation{mutation},format{format:?}"
            );
        }
        assert!(exported_asset_from_bytes(&req, &png(4, 4)).is_err());
    }
}
