use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp_figma::{
    AssetExportOutcome, AssetFormat, AssetRenderMode, AssetRequest, AssetSelection, RawNode,
    Snapshot, UpstreamResult, asset_export_from_result, resolve_asset_selections,
};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Cursor, process::Command};
fn request() -> AssetRequest {
    AssetRequest {
        usage_aspect_ratio: None,
        max_width: None,
        srcset: false,
        visible_scope_node_id: None,
        quality: None,
        lossless: None,
        render_mode: AssetRenderMode::OriginalSource,
        asset_id: "picture:fills:0".into(),
        node_id: "picture".into(),
        field: "fills/0".into(),
        image_hash: Some("source".into()),
        format: AssetFormat::Png,
        scale: 2.0,
    }
}
fn encoded(w: u32, h: u32, format: ImageFormat) -> Vec<u8> {
    let rgba = RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            (x % 255) as u8,
            (y % 255) as u8,
            127,
            if x % 2 == 0 { 0 } else { 179 },
        ])
    });
    let mut out = Cursor::new(Vec::new());
    if format == ImageFormat::Jpeg {
        DynamicImage::ImageRgba8(rgba)
            .to_rgb8()
            .write_to(&mut out, format)
            .unwrap();
    } else {
        DynamicImage::ImageRgba8(rgba)
            .write_to(&mut out, format)
            .unwrap();
    }
    out.into_inner()
}
fn decode(
    req: &AssetRequest,
    bytes: &[u8],
    node_width: f64,
) -> Result<devup_mcp_figma::AssetManifestEntry, devup_mcp_figma::DevupError> {
    let hash = Sha256::digest(bytes)
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect::<String>();
    let raw = json!({"kind":"devupAssetExport","fileKey":"fixture","version":null,"assetId":req.asset_id,"nodeId":req.node_id,"field":req.field,"imageHash":req.image_hash,"format":req.format,"scale":req.scale,"renderMode":req.render_mode,"maxWidth":req.max_width,"srcset":req.srcset,"visibleScopeNodeId":req.visible_scope_node_id,"usageAspectRatio":req.usage_aspect_ratio,"status":"exported","byteLength":bytes.len(),"sha256":hash,"mimeType":image::guess_format(bytes).unwrap().to_mime_type(),"data":STANDARD.encode(bytes),"exportDiagnostics":{"method":"original-fill","stages":[],"renderInfo":{"representation":"original-fill","nodeSize":{"width":node_width,"height":80.0,"aspectRatio":node_width/80.0},"sourceProperties":{"scaleMode":"CROP","rotation":0,"effects":[{"type":"LAYER_BLUR","radius":12}]}}}});
    match asset_export_from_result(&UpstreamResult { raw }, "fixture", None, req)? {
        AssetExportOutcome::Entry(entry) => Ok(*entry),
        _ => panic!("entry"),
    }
}
fn snapshot() -> Snapshot {
    let nodes=[json!({"id":"root","type":"FRAME","fields":{"name":"Root","width":400,"height":300,"childrenIds":["card"]},"extra":{},"fieldErrors":{}}),json!({"id":"card","type":"INSTANCE","fields":{"name":"Card","width":240,"height":150,"childrenIds":["picture"],"clipsContent":true},"extra":{},"fieldErrors":{}}),json!({"id":"picture","type":"RECTANGLE","fields":{"name":"Picture","width":120,"height":80,"fills":[{"type":"IMAGE","imageHash":"source","scaleMode":"CROP"}]},"extra":{},"fieldErrors":{}}),json!({"id":"outside","type":"FRAME","fields":{"name":"Context","width":50,"height":50},"extra":{},"fieldErrors":{}})].into_iter().map(|v|{let n:RawNode=serde_json::from_value(v).unwrap();(n.id.clone(),n)}).collect::<BTreeMap<_,_>>();
    Snapshot {
        file_key: "fixture".into(),
        version: None,
        roots: vec!["root".into()],
        nodes,
        diagnostics: Vec::new(),
    }
}
fn selection(id: &str, mode: AssetRenderMode) -> AssetSelection {
    AssetSelection {
        usage_aspect_ratio: None,
        max_width: None,
        srcset: false,
        visible_scope_node_id: None,
        quality: None,
        lossless: None,
        asset_id: id.into(),
        format: AssetFormat::Png,
        scale: 1.5,
        render_mode: mode,
    }
}
#[test]
fn compiled_and_source_modes_match() {
    let out = Command::new("node")
        .arg("--test")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/original_source_script.mjs"
        ))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
#[test]
fn original_png_is_full_identity_at_fractional_scale_and_never_bakes() {
    let bytes = encoded(400, 200, ImageFormat::Png);
    let mut req = request();
    req.scale = 1.5;
    let entry = decode(&req, &bytes, 120.0).unwrap();
    assert_eq!(STANDARD.decode(entry.data_base64.unwrap()).unwrap(), bytes);
    let i = entry.render_info.unwrap();
    assert_eq!(i["sourcePixelSize"]["width"], 400.0);
    assert_eq!(i["outputSize"]["width"], 400.0);
    assert_eq!(i["nodeBox"]["width"], 120.0);
    assert_eq!(i["cropApplied"], false);
    assert_eq!(i["effectsBaked"], false);
    assert_eq!(i["effectsAppliedBy"], "css");
    assert_eq!(i["fidelityScope"], "original-source-bytes");
}
#[test]
fn jpeg_to_png_is_explicit_full_source_conversion() {
    let bytes = encoded(30, 20, ImageFormat::Jpeg);
    let e = decode(&request(), &bytes, 120.0).unwrap();
    let output = STANDARD.decode(e.data_base64.unwrap()).unwrap();
    assert_eq!(image::guess_format(&output).unwrap(), ImageFormat::Png);
    assert_eq!(
        image::load_from_memory(&output).unwrap().to_rgb8(),
        image::load_from_memory(&bytes).unwrap().to_rgb8()
    );
    let i = e.render_info.unwrap();
    assert_eq!(i["representation"], "original-fill-transcoded");
    assert_eq!(
        i["fidelityScope"],
        "decoded-full-original-source-explicit-codec-conversion; source-byte-identity-not-preserved"
    );
    assert!(
        !i["fidelityScope"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("webp")
    );
    assert_eq!(i["compression"]["input"]["mimeType"], "image/jpeg");
    assert_eq!(i["compression"]["outputMimeType"], "image/png");
    assert!(i["compression"]["method"].is_null());
    assert!(
        i["warnings"]
            .as_array()
            .unwrap()
            .contains(&json!("derivative-source-metadata-not-preserved"))
    );
    assert_eq!(i["effectsBaked"], false);
}
#[test]
fn resize_and_srcset_preserve_actual_density_alpha_and_bound_identity() {
    let bytes = encoded(800, 500, ImageFormat::Png);
    let mut req = request();
    req.format = AssetFormat::Webp;
    req.lossless = Some(true);
    req.max_width = Some(300);
    req.srcset = true;
    req.usage_aspect_ratio = Some(1.0);
    let e = decode(&req, &bytes, 120.1).unwrap();
    let base = image::load_from_memory(&STANDARD.decode(e.data_base64.as_ref().unwrap()).unwrap())
        .unwrap();
    assert_eq!(base.width(), 121);
    assert_eq!(e.variants.len(), 1);
    let v = &e.variants[0];
    assert_eq!(v.width, 241);
    assert!((v.density - 241.0 / 121.0).abs() < 1e-10);
    let twice = image::load_from_memory(&STANDARD.decode(v.data_base64.as_ref().unwrap()).unwrap())
        .unwrap();
    assert_eq!(twice.width(), 241);
    assert!(twice.to_rgba8().pixels().any(|p| p[3] != 255));
    let i = e.render_info.unwrap();
    assert_eq!(i["sourcePixelSize"]["width"], 800);
    assert_eq!(i["resize"]["upscaled"], false);
    assert_eq!(i["compression"]["quality"], 85);
    assert_eq!(
        i["compression"]["lossless"], true,
        "lossless encoder mode is retained even though resizing makes a derivative"
    );
    assert!(i["compression"]["sourceSavingsPercent"].is_number());
    assert_eq!(i["compression"]["sourcePixelsPreserved"], false);
    assert_eq!(i["fidelity"], "approximate");
    assert_eq!(e.max_width, req.max_width);
    assert_eq!(e.usage_aspect_ratio, Some(1.0));
}
#[test]
fn source_cap_never_creates_duplicate_or_upscaled_srcset() {
    let bytes = encoded(60, 40, ImageFormat::Png);
    let mut req = request();
    req.format = AssetFormat::Webp;
    req.max_width = Some(400);
    req.srcset = true;
    let e = decode(&req, &bytes, 120.1).unwrap();
    assert_eq!(e.render_info.as_ref().unwrap()["outputSize"]["width"], 60.0);
    assert!(e.variants.is_empty());
    assert!(
        e.render_info.unwrap()["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w == "source-resolution-below-1.5x-display")
    );
}
#[test]
fn arbitrary_parent_and_visible_scope_require_current_acquired_ancestry() {
    let snap = snapshot();
    let parent =
        resolve_asset_selections(&snap, &[selection("card:node", AssetRenderMode::Node)]).unwrap();
    assert_eq!(parent[0].node_id, "card");
    assert!(
        resolve_asset_selections(&snap, &[selection("outside:node", AssetRenderMode::Node)])
            .is_err()
    );
    let visible = resolve_asset_selections(
        &snap,
        &[selection("picture:fills:0", AssetRenderMode::Visible)],
    )
    .unwrap();
    assert_eq!(visible[0].visible_scope_node_id.as_deref(), Some("card"));
    let mut wrong = selection("picture:fills:0", AssetRenderMode::Visible);
    wrong.visible_scope_node_id = Some("outside".into());
    assert!(resolve_asset_selections(&snap, &[wrong]).is_err());
    let mut leaf = snap;
    leaf.roots = vec!["picture".into()];
    assert!(
        resolve_asset_selections(
            &leaf,
            &[selection("picture:fills:0", AssetRenderMode::Visible)]
        )
        .is_err()
    );
}
#[test]
fn invalid_scales_dimensions_and_legacy_resize_fail_before_export() {
    for scale in [f64::NAN, f64::INFINITY, -1.0, 0.0, 4.1] {
        let mut s = selection("picture:fills:0", AssetRenderMode::Node);
        s.scale = scale;
        assert!(resolve_asset_selections(&snapshot(), &[s]).is_err());
    }
    for width in [0, 8193] {
        let mut s = selection("picture:fills:0", AssetRenderMode::OriginalSource);
        s.format = AssetFormat::Webp;
        s.max_width = Some(width);
        assert!(resolve_asset_selections(&snapshot(), &[s]).is_err());
    }
    let mut s = selection("picture:fills:0", AssetRenderMode::Fill);
    s.scale = 1.0;
    s.format = AssetFormat::Webp;
    s.max_width = Some(100);
    assert!(resolve_asset_selections(&snapshot(), &[s]).is_err());
}
#[test]
fn four_k_source_is_within_bounded_webp_pixel_budget() {
    let mut out = Cursor::new(Vec::new());
    DynamicImage::new_rgba8(4096, 4096)
        .write_to(&mut out, ImageFormat::Png)
        .unwrap();
    let mut req = request();
    req.format = AssetFormat::Webp;
    req.max_width = Some(256);
    let e = decode(&req, out.get_ref(), 120.0).unwrap();
    assert_eq!(
        e.render_info.as_ref().unwrap()["sourcePixelSize"]["width"],
        4096
    );
    assert_eq!(e.render_info.unwrap()["outputSize"]["width"], 256.0);
}
#[test]
fn original_mode_default_is_explicit_and_serialized() {
    assert_eq!(AssetRenderMode::default(), AssetRenderMode::OriginalSource);
    assert_eq!(
        serde_json::to_value(AssetRenderMode::default()).unwrap(),
        json!("original-source")
    );
    let _: Value = serde_json::to_value(request()).unwrap();
}

#[test]
fn explicit_conversion_applies_exif_orientation_and_matching_source_keeps_bytes() {
    let jpeg = encoded(30, 20, ImageFormat::Jpeg);
    // APP1 Exif: little-endian TIFF IFD with Orientation SHORT=6.
    let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
    let mut source = vec![0xff, 0xd8, 0xff, 0xe1];
    source.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
    source.extend_from_slice(exif);
    source.extend_from_slice(&jpeg[2..]);
    let mut req = request();
    req.format = AssetFormat::Jpg;
    let raw = decode(&req, &source, 120.0).unwrap();
    assert_eq!(STANDARD.decode(raw.data_base64.unwrap()).unwrap(), source);
    req.format = AssetFormat::Webp;
    req.lossless = Some(true);
    let converted = decode(&req, &source, 120.0).unwrap();
    let bytes = STANDARD.decode(converted.data_base64.unwrap()).unwrap();
    let actual = image::load_from_memory(&bytes).unwrap();
    let mut expected = image::load_from_memory(&jpeg).unwrap();
    expected.apply_orientation(image::metadata::Orientation::Rotate90);
    assert_eq!((actual.width(), actual.height()), (20, 30));
    assert_eq!(actual.to_rgba8(), expected.to_rgba8());
    assert_eq!(
        converted.render_info.as_ref().unwrap()["orientation"]["exif"],
        6
    );
    assert_eq!(
        converted.render_info.unwrap()["orientation"]["appliedToPixels"],
        true
    );
}
