use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp_figma::{
    AssetExportOutcome, AssetFormat, AssetRequest, AssetStatus, ReadToolCall, UpstreamResult,
    asset_export_from_result,
};
use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn vector() -> Value {
    json!({"id":"I44:8533;10:973", "type":"VECTOR", "opacity":0.8,
        "absoluteRenderBounds":{"x":10,"y":20,"width":32,"height":24},
        "absoluteTransform":[[1,0,10],[0,1,20]], "relativeTransform":[[0,-1,15],[1,0,2]],
        "fills":[{"type":"SOLID","color":{"r":0.25,"g":0.5,"b":0.75},"opacity":0.6}],
        "fillGeometry":[{"windingRule":"EVENODD","data":"M0 0H32V24H0Z M4 4H8V8H4Z"}],
        "strokes":[], "effects":[]})
}

fn execute(node: Value, format: AssetFormat, mode: &str, writer: bool) -> (AssetRequest, Value) {
    let request = AssetRequest {
        render_mode: devup_mcp_figma::AssetRenderMode::Node,
        quality: None,
        lossless: None,
        asset_id: "I44:8533;10:973:node".into(),
        node_id: "I44:8533;10:973".into(),
        field: "node".into(),
        image_hash: None,
        format,
        scale: 2,
    };
    let call = ReadToolCall::asset_export("fixture", None, request.clone());
    let result = run(call.arguments()["code"].clone(), node, mode, writer);
    (request, result)
}

fn run(code: Value, node: Value, mode: &str, writer: bool) -> Value {
    let input = json!({"code":code,"node":node,"mode":mode,"writer":writer});
    let js = r#"
const input = JSON.parse(require('node:fs').readFileSync(0,'utf8'));
const node = input.node;
let exports = 0, writes = 0, lookups = [];
node.exportAsync = async settings => {
  exports++;
  if(input.mode === 'export-timeout') return new Promise(()=>{});
  if(input.mode === 'late-native') return new Promise(resolve=>setTimeout(()=>resolve('<svg><path d="M999 999Z"/></svg>'),1200));
  if(input.mode !== 'native') throw new Error('renderer exploded on nested instance');
  return '<svg xmlns="http://www.w3.org/2000/svg"><path d="M1 2L3 4" fill="red"/><mask id="original-mask"/></svg>';
};
const figma = { fileKey:'fixture', editorType:'dev',
 base64Encode:b=>Buffer.from(b).toString('base64'),
 getNodeByIdAsync:async id=>{
   lookups.push(id);
   if(input.mode === 'lookup-timeout') return new Promise(()=>{});
   if(input.mode === 'lookup-fallback' && id.startsWith('I')) throw new Error('nested lookup failed');
   if(id === '44:8533') return {id, children:[{id:'sibling',children:[]},node]};
   return node;
 }};
if(input.writer) figma.io={write:()=>writes++};
const AsyncFunction=Object.getPrototypeOf(async function(){}).constructor;
const code=input.code.replace(/"timeoutMs":\d+/, '"timeoutMs":1000');
new AsyncFunction('figma',code)(figma).then(data=>process.stdout.write(JSON.stringify({data,exports,writes,lookups}))).catch(e=>{console.error(e);process.exitCode=1});
"#;
    let mut child = Command::new("node")
        .args(["-e", js])
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
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn decoded(request: &AssetRequest, result: &Value) -> devup_mcp_figma::AssetManifestEntry {
    let result = UpstreamResult {
        raw: json!({"content":[{"type":"text","text":result["data"].to_string()}]}),
    };
    let AssetExportOutcome::Entry(entry) =
        asset_export_from_result(&result, "fixture", None, request).unwrap()
    else {
        panic!("expected entry")
    };
    *entry
}

#[test]
fn nested_vector_fallback_preserves_real_paths_color_opacity_and_holes() {
    let (request, result) = execute(vector(), AssetFormat::Svg, "failed", false);
    let asset = decoded(&request, &result);
    assert_eq!(asset.status, AssetStatus::Exported);
    let svg = String::from_utf8(STANDARD.decode(asset.data_base64.unwrap()).unwrap()).unwrap();
    let mut reader = quick_xml::Reader::from_str(&svg);
    while !matches!(reader.read_event().unwrap(), quick_xml::events::Event::Eof) {}
    for fragment in [
        "<svg ",
        "viewBox=\"0 0 32 24\"",
        "M0 0H32V24H0Z M4 4H8V8H4Z",
        "fill-rule=\"evenodd\"",
        "rgb(25%,50%,75%)",
        "opacity=\"0.8\"",
        "fill-opacity=\"0.6\"",
    ] {
        assert!(svg.contains(fragment), "{svg}");
    }
    assert_eq!(result["exports"], 1);
    assert_eq!(result["writes"], 0);
}

#[test]
fn nested_lookup_and_group_fallback_use_only_the_requested_subtree() {
    let mut root = vector();
    root["type"] = json!("GROUP");
    root["fills"] = json!([]);
    let mut child = vector();
    child["id"] = json!("child");
    root["children"] = json!([child]);
    let (request, result) = execute(root, AssetFormat::Svg, "lookup-fallback", false);
    let asset = decoded(&request, &result);
    assert_eq!(asset.status, AssetStatus::Exported);
    assert_eq!(result["lookups"], json!(["I44:8533;10:973", "44:8533"]));
    let svg = String::from_utf8(STANDARD.decode(asset.data_base64.unwrap()).unwrap()).unwrap();
    assert!(svg.contains("matrix(0 1 -1 0 15 2)"));
    assert_eq!(
        svg.matches("<path ").count(),
        1,
        "siblings must never enter the export"
    );
}

#[test]
fn unsupported_fidelity_cases_report_the_native_error_and_write_nothing() {
    for (field, value, reason) in [
        ("isMask", json!(true), "mask"),
        ("clipsContent", json!(true), "clipping"),
        ("fills", json!([{"type":"GRADIENT_LINEAR"}]), "paint type"),
        ("effects", json!([{"type":"DROP_SHADOW"}]), "effect"),
        ("type", json!("TEXT"), "node type"),
        (
            "absoluteTransform",
            json!([[0, -1, 0], [1, 0, 0]]),
            "rotation",
        ),
    ] {
        let mut node = vector();
        node[field] = value;
        let (request, result) = execute(node, AssetFormat::Svg, "failed", true);
        let asset = decoded(&request, &result);
        assert_eq!(asset.status, AssetStatus::Failed, "{field}: {result}");
        let detail = asset.error_detail.unwrap();
        assert!(detail.contains("renderer exploded"), "{detail}");
        assert!(detail.contains(reason), "{detail}");
        assert!(asset.path.is_none() && asset.data_base64.is_none());
        assert_eq!(result["writes"], 0);
    }
    let mut stroke = vector();
    stroke["strokes"] = stroke["fills"].clone();
    stroke["strokeAlign"] = json!("INSIDE");
    let (_, result) = execute(stroke, AssetFormat::Svg, "failed", false);
    assert!(
        result["data"]["errorDetail"]
            .as_str()
            .unwrap()
            .contains("non-center stroke")
    );
}

#[test]
fn native_mask_export_is_unchanged_and_binary_failures_never_become_svg() {
    let mut node = vector();
    node["isMask"] = json!(true);
    let (request, result) = execute(node, AssetFormat::Svg, "native", false);
    let asset = decoded(&request, &result);
    let bytes = STANDARD.decode(asset.data_base64.unwrap()).unwrap();
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M1 2L3 4\" fill=\"red\"/><mask id=\"original-mask\"/></svg>"
    );
    for format in [AssetFormat::Png, AssetFormat::Pdf] {
        let (request, result) = execute(vector(), format, "failed", false);
        assert_eq!(decoded(&request, &result).status, AssetStatus::Failed);
        assert!(result["data"].get("text").is_none());
    }
}

#[test]
fn lookup_and_export_timeouts_are_distinct_and_bounded() {
    for (mode, stage) in [("lookup-timeout", "lookup"), ("export-timeout", "export")] {
        let mut node = vector();
        node["isMask"] = json!(true);
        let (request, result) = execute(node, AssetFormat::Svg, mode, false);
        let asset = decoded(&request, &result);
        assert_eq!(asset.status, AssetStatus::Failed);
        assert_eq!(asset.error_code.as_deref(), Some("DEVUP_ASSET_TIMEOUT"));
        let detail = asset.error_detail.unwrap();
        assert!(
            detail.starts_with(stage) && detail.contains("DEVUP_ASSET_TIMEOUT:"),
            "{detail}"
        );
        assert_eq!(result["writes"], 0);
    }
}

#[test]
fn hung_renderer_can_fallback_and_retains_its_real_timeout_evidence() {
    let (request, result) = execute(vector(), AssetFormat::Svg, "export-timeout", false);
    let asset = decoded(&request, &result);
    assert_eq!(asset.status, AssetStatus::Exported);
    let diagnostics = asset.export_diagnostics.unwrap();
    assert_eq!(diagnostics.method, "geometry");
    assert!(
        diagnostics
            .native_error
            .unwrap()
            .contains("DEVUP_ASSET_TIMEOUT")
    );
    assert!(
        diagnostics
            .stages
            .iter()
            .any(|s| s.stage == "native-export" && s.elapsed_ms >= 790)
    );
    assert!(
        diagnostics
            .stages
            .iter()
            .any(|s| s.stage == "geometry-fallback")
    );
    assert_eq!(result["exports"], 1);
}

#[test]
fn a_late_native_answer_cannot_overwrite_the_validated_fallback() {
    let (request, result) = execute(vector(), AssetFormat::Svg, "late-native", true);
    let asset = decoded(&request, &result);
    assert_eq!(asset.status, AssetStatus::Exported);
    let svg = String::from_utf8(STANDARD.decode(asset.data_base64.unwrap()).unwrap()).unwrap();
    assert!(svg.contains("M0 0H32V24H0Z") && !svg.contains("M999"));
    assert_eq!(
        result["writes"], 1,
        "only the winning geometry payload is written"
    );
}

#[test]
fn fragment_reexport_uses_the_same_fallback_and_keeps_real_failure_details() {
    use devup_mcp_figma::LargeValueReadOptions;
    let (request, result) = execute(vector(), AssetFormat::Svg, "failed", false);
    let asset = decoded(&request, &result);
    let code = ReadToolCall::large_value(
        "fixture",
        LargeValueReadOptions {
            node_id: request.node_id.clone(),
            field: "$export:svg".into(),
            offset: 0,
            max_chunk_bytes: 65536,
            byte_length: asset.byte_length.unwrap(),
            sha256: asset.sha256.unwrap(),
            version: None,
        },
    )
    .arguments()["code"]
        .clone();
    let fragment = run(code.clone(), vector(), "failed", true);
    assert_eq!(fragment["data"]["kind"], "devupLargeValueFragment");
    assert_eq!(
        STANDARD
            .decode(fragment["data"]["dataBase64"].as_str().unwrap())
            .unwrap(),
        STANDARD.decode(asset.data_base64.unwrap()).unwrap()
    );
    let mut node = vector();
    node["isMask"] = json!(true);
    let failed = run(code, node, "failed", true);
    assert_eq!(failed["data"]["kind"], "devupLargeValueUnsupported");
    let detail = failed["data"]["errorDetail"].as_str().unwrap();
    assert!(detail.contains("renderer exploded") && detail.contains("mask"));
    assert_eq!(failed["writes"], 0);
}
