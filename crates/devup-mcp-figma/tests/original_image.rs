use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp_figma::{AssetFormat, AssetRequest, ReadToolCall};
use serde_json::json;
use sha2::{Digest, Sha256};

#[test]
fn original_descriptor_is_verified_without_claiming_a_png_rendition() {
    let request = AssetRequest::original_image("n", 2, "hash");
    let bytes = [255u8, 216, 255, 224, 1, 2];
    let hash: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let descriptor = json!({"kind":"devupOriginalImage", "representation":"original-image-v1",
        "fileKey":"file", "version":"v1", "nodeId":"n", "assetId":request.asset_id,
        "field":request.field, "imageHash":"hash", "status":"exported",
        "format":null, "scale":null, "mimeType":"image/jpeg", "width":13, "height":17,
        "byteLength":bytes.len(), "sha256":hash, "data":STANDARD.encode(bytes)});
    let decode = |value: serde_json::Value| {
        devup_mcp_figma::original_image_from_result(
            &devup_mcp_figma::UpstreamResult {
                raw: json!({"content":[{"type":"text","text":value.to_string()}]}),
            },
            "file",
            Some("v1"),
            &request,
        )
    };
    let original = decode(descriptor.clone()).unwrap();
    assert_eq!(original.bytes, bytes);
    assert_eq!(original.mime_type, "image/jpeg");
    assert_eq!((original.width, original.height), (13, 17));
    for (field, bad) in [
        ("imageHash", json!("other")),
        ("version", json!("v2")),
        ("sha256", json!("bad")),
        ("byteLength", json!(2)),
        ("mimeType", json!("image/png")),
        ("width", json!(0)),
        ("representation", json!("node-render")),
    ] {
        let mut altered = descriptor.clone();
        altered[field] = bad;
        assert!(decode(altered).is_err(), "must reject {field}");
    }
}

#[test]
fn original_image_script_contract() {
    let output = std::process::Command::new("node")
        .arg("--test")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/original_image_script.mjs"
        ))
        .output()
        .expect("Node is required for the Plugin API contract");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn original_bytes_are_opted_in_only_by_the_bridge_transport() {
    let call = ReadToolCall::asset_export(
        "file",
        Some("v1".into()),
        AssetRequest {
            asset_id: "n:original:2".into(),
            node_id: "n".into(),
            field: "$original-image/fills/2".into(),
            image_hash: Some("active".into()),
            format: AssetFormat::Png,
            scale: 1,
        },
    );
    assert_eq!(
        call.bridge_job().unwrap().params["asset"]["transport"],
        "bridge"
    );
    assert!(
        !call.arguments()["code"]
            .as_str()
            .unwrap()
            .contains("\"transport\":\"bridge\"")
    );
}
