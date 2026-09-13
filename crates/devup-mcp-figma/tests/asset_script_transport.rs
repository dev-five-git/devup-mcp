use std::io::Write;
use std::process::{Command, Stdio};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp_figma::{
    AssetExportOutcome, AssetFormat, AssetRequest, AssetStatus, ReadToolCall, UpstreamResult,
    asset_export_from_result,
};
use serde_json::{Value, json};

fn execute(
    format: AssetFormat,
    length: usize,
    writer: bool,
    export_fails: bool,
) -> (AssetRequest, Value) {
    let request = AssetRequest {
        asset_id: "1:2:node".to_owned(),
        node_id: "1:2".to_owned(),
        field: "node".to_owned(),
        image_hash: None,
        format,
        scale: 2,
    };
    let call = ReadToolCall::asset_export("fixture", Some("v1".to_owned()), request.clone());
    let code = call.arguments()["code"].as_str().unwrap().to_owned();
    let input =
        json!({"code": code, "length": length, "writer": writer, "exportFails": export_fails});
    // The native Plugin API provides base64Encode, but no figma.io. Keep the
    // source bytes deterministic and exercise the actual compiled export script
    // and Rust response decoder, including JSON nested in the bridge envelope.
    let js = r#"
const input = JSON.parse(require('node:fs').readFileSync(0, 'utf8'));
const bytes = Uint8Array.from({length: input.length}, (_, i) => 65 + i % 26);
let exports = 0, writes = 0;
const figma = {
  fileKey: 'fixture',
  base64Encode: value => Buffer.from(value).toString('base64'),
  getNodeByIdAsync: async () => ({exportAsync: async settings => {
    exports++;
    if (input.exportFails) throw new Error('renderer failed');
    return settings.format === 'SVG_STRING' ? Buffer.from(bytes).toString() : bytes;
  }}),
};
if (input.writer) figma.io = {write: () => { writes++; }};
const AsyncFunction = Object.getPrototypeOf(async function(){}).constructor;
new AsyncFunction('figma', input.code)(figma).then(data => {
  process.stdout.write(JSON.stringify({exports, writes, data}));
}).catch(error => { console.error(error); process.exitCode = 1; });
"#;
    let mut child = Command::new("node")
        .args(["-e", js])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Node is required to execute the asset script contract");
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
    (request, serde_json::from_slice(&output.stdout).unwrap())
}

#[test]
fn native_plugin_exports_without_a_remote_file_writer() {
    for (format, length) in [
        (AssetFormat::Png, 7),
        (AssetFormat::Png, 800_000),
        (AssetFormat::Jpg, 257),
        (AssetFormat::Pdf, 258),
        (AssetFormat::Svg, 11),
        (AssetFormat::Svg, 13_000),
    ] {
        let (request, result) = execute(format, length, false, false);
        assert_eq!(result["exports"], 1);
        assert_eq!(result["writes"], 0);
        let response = UpstreamResult {
            raw: json!({"content": [{"type": "text", "text": result["data"].to_string()}]}),
        };
        let AssetExportOutcome::Entry(asset) =
            asset_export_from_result(&response, "fixture", Some("v1"), &request).unwrap()
        else {
            panic!("a bridge response can carry the bounded bytes without re-exporting fragments")
        };
        assert_eq!(
            asset.status,
            AssetStatus::Exported,
            "{format:?}/{length}: {result}"
        );
        let expected: Vec<u8> = (0..length).map(|i| 65 + (i % 26) as u8).collect();
        assert_eq!(
            STANDARD.decode(asset.data_base64.unwrap()).unwrap(),
            expected
        );
        assert_eq!(asset.byte_length, Some(length));
        assert_eq!(asset.mime_type.as_deref(), Some(format.mime_type()));
    }
}

#[test]
fn remote_asset_delivery_retains_attachment_and_fragment_limits() {
    for (format, length, status, writes) in [
        (AssetFormat::Png, 7, "exported", 1),
        (AssetFormat::Png, 800_000, "chunked", 0),
        (AssetFormat::Svg, 11, "exported", 1),
        (AssetFormat::Svg, 13_000, "chunked", 0),
    ] {
        let (_, result) = execute(format, length, true, false);
        assert_eq!(result["data"]["status"], status);
        assert_eq!(result["exports"], 1);
        assert_eq!(result["writes"], writes);
    }
}

#[test]
fn bridge_does_not_hide_export_failure_or_bypass_byte_limit() {
    for (length, fails, error) in [
        (3, true, "DEVUP_ASSET_EXPORT_FAILED"),
        (0, false, "DEVUP_ASSET_RESPONSE_TOO_LARGE"),
        (8 * 1024 * 1024 + 1, false, "DEVUP_ASSET_RESPONSE_TOO_LARGE"),
    ] {
        let (_, result) = execute(AssetFormat::Png, length, false, fails);
        assert_eq!(result["data"]["status"], "failed");
        assert_eq!(result["data"]["errorCode"], error);
        assert_eq!(result["writes"], 0);
    }
}
