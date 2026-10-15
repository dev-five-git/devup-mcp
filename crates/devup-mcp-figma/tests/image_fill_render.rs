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
        asset_id: "sample:fills:0".into(),
        node_id: "sample".into(),
        field: "fills/0".into(),
        image_hash: Some("source".into()),
        format: AssetFormat::Png,
        scale: 1,
        render_mode: mode,
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
        assert_eq!(info["fidelity"], "unverified");
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
    request.scale = 3;
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
fn selection_defaults_to_node_and_refuses_unsupported_fill_settings() {
    let default: AssetSelection =
        serde_json::from_value(json!({"assetId":"sample:fills:0","format":"png","scale":1}))
            .unwrap();
    assert_eq!(default.render_mode, AssetRenderMode::Node);
    let fill = AssetSelection {
        render_mode: AssetRenderMode::Fill,
        ..default
    };
    assert_eq!(
        resolve_asset_selections(&snapshot(), std::slice::from_ref(&fill)).unwrap()[0].render_mode,
        AssetRenderMode::Fill
    );
    for selection in [
        AssetSelection {
            scale: 2,
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
