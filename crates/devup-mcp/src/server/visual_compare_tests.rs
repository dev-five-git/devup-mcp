use super::*;
async fn run(
    input: Value,
    policy: &OutputPolicy,
    store: &ArtifactStore,
) -> Result<Value, DevupError> {
    let input =
        serde_json::from_value(input).map_err(|_| invalid("Invalid visual comparison input."))?;
    compare(input, policy, store).await
}
use serde_json::json;

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "visual-tool-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn png(&self, name: &str, png: &str) {
        std::fs::write(self.0.join(name), STANDARD.decode(png).unwrap()).unwrap();
    }
    fn policy(&self) -> OutputPolicy {
        OutputPolicy::from_roots(vec![self.0.clone()]).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn reference_hash() -> String {
    hash(&STANDARD.decode(WHITE).unwrap())
}
fn environment() -> Value {
    json!({"schemaVersion":1,"tsxResource":{"uri":"devup://artifact/test/manifest","sha256":"a".repeat(64)},"referenceResource":{"uri":"devup://artifact/test/manifest","sha256":reference_hash()},"viewport":{"width":1,"height":1,"deviceScaleFactor":1},"captureContext":{"innerWidth":1,"innerHeight":1,"devicePixelRatio":1,"visualViewportScale":1,"rootCssZoom":1,"bodyCssZoom":1},"themePath":"devup.json","assetDirectory":"public/assets","fontManifest":[{"family":"Test","sha256":"b".repeat(64)}],"renderer":{"name":"playwright-chromium","version":"1"},"devupUiVersion":"1","actualPng":"actual.png","environmentStatus":"valid","os":"test","locale":"en-US","timezone":"UTC"})
}
fn input() -> Value {
    json!({"actual":"actual.png","reference":{"path":"reference.png"},"environment":environment()})
}
const WHITE: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4////fwAJ+wP9KobjigAAAABJRU5ErkJggg==";
const BLACK: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNgYGD4DwABBAEAX+XDSwAAAABJRU5ErkJggg==";
const WIDE: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAC0lEQVR4nGP4DwUAI+UH+Yo0eLMAAAAASUVORK5CYII=";
fn fixture() -> Fixture {
    let f = Fixture::new();
    f.png("actual.png", WHITE);
    f.png("reference.png", WHITE);
    f
}
#[tokio::test]
async fn dimension_mismatch_is_inconclusive_not_a_codegen_failure() {
    let f = fixture();
    f.png("actual.png", WIDE);
    let result = run(input(), &f.policy(), &ArtifactStore::default())
        .await
        .unwrap();
    assert_eq!(result["verdict"], "inconclusive");
    assert_eq!(result["visual"]["status"], "invalid-dimensions");
    assert!(
        result["visual"]["changedRatio"].is_null(),
        "no pixel mismatch percentage was measured"
    );
}
#[tokio::test]
async fn environment_overrides_exact_pixels() {
    let f = fixture();
    let mut i = input();
    i["environment"]["environmentStatus"] = json!("environment-invalid");
    let r = run(i, &f.policy(), &ArtifactStore::default())
        .await
        .unwrap();
    assert_eq!(r["visual"]["passed"], true);
    assert_eq!(r["environment"]["status"], "environment-invalid");
    assert_eq!(r["verdict"], "inconclusive");
}
#[tokio::test]
async fn missing_environment_inconclusive() {
    let f = fixture();
    let mut i = input();
    i.as_object_mut().unwrap().remove("environment");
    let r = run(i, &f.policy(), &ArtifactStore::default())
        .await
        .unwrap();
    assert_eq!(r["verdict"], "inconclusive");
}

async fn asset_store(representation: &str, broken_hash: bool) -> (ArtifactStore, String) {
    let store = ArtifactStore::default();
    let mut payload = super::super::artifacts::w3_tests::payload();
    let bytes = STANDARD.decode(WHITE).unwrap();
    payload.assets.push(serde_json::from_value(json!({"assetId":"picture:fills:0","nodeId":"picture","field":"fills/0","sourceKind":"image-fill",
            "format":"png","scale":1,"status":"exported","mimeType":"image/png","byteLength":bytes.len(),"dataBase64":WHITE,
            "sha256":if broken_hash {"0".repeat(64)}else{hash(&bytes)},"renderInfo":{"representation":representation}})).unwrap());
    let artifact = store
        .insert(
            ArtifactRequestKey::from_collection(&super::super::artifacts::w3_tests::request()),
            payload,
        )
        .await
        .unwrap();
    (store, artifact.artifact_id.clone())
}

#[tokio::test]
async fn asset_comparison_has_validated_native_reference_and_pixel_scope() {
    let f = fixture();
    let (store, id) = asset_store("native-node", false).await;
    let i = json!({"actual":"actual.png","reference":{"artifactId":id,"assetId":"picture:fills:0"},"threshold":0});
    let matched = run(i.clone(), &f.policy(), &store).await.unwrap();
    assert_eq!(matched["verdict"], "pass");
    assert_eq!(matched["reference"]["assetId"], "picture:fills:0");
    assert_eq!(matched["reference"]["referenceSha256"], reference_hash());
    assert_eq!(matched["actual"]["sha256"], reference_hash());
    assert_eq!(
        matched["comparisonScope"],
        "asset-pixels-against-native-node-export"
    );
    f.png("actual.png", BLACK);
    let mismatch = run(i.clone(), &f.policy(), &store).await.unwrap();
    assert_eq!(mismatch["verdict"], "fail");
    assert_eq!(mismatch["visual"]["changedRatio"], 1.0);
    assert_eq!(mismatch["visual"]["totalPixels"], 1);
    f.png("actual.png", WIDE);
    assert!(
        run(i.clone(), &f.policy(), &store)
            .await
            .unwrap_err()
            .message
            .contains("dimensions")
    );
    std::fs::write(f.0.join("actual.png"), b"invalid PNG").unwrap();
    assert!(
        run(i, &f.policy(), &store)
            .await
            .unwrap_err()
            .message
            .contains("Invalid PNG")
    );
}

#[tokio::test]
async fn webp_actual_uses_native_png_reference_and_keeps_original_byte_hash_and_scope() {
    let f = fixture();
    let (store, id) = asset_store("native-node", false).await;
    let mut renditions = Vec::new();
    for (png, verdict) in [(WHITE, "pass"), (BLACK, "fail")] {
        let request = devup_mcp_figma::AssetRequest {
            usage_aspect_ratio: None,
            max_width: None,
            srcset: false,
            visible_scope_node_id: None,
            asset_id: "picture:fills:0".into(),
            node_id: "picture".into(),
            field: "fills/0".into(),
            image_hash: Some("source".into()),
            format: devup_mcp_figma::AssetFormat::Webp,
            scale: 1.0,
            render_mode: devup_mcp_figma::AssetRenderMode::Node,
            quality: Some(80),
            lossless: Some(true),
        };
        let asset =
            devup_mcp_figma::exported_asset_from_bytes(&request, &STANDARD.decode(png).unwrap())
                .unwrap();
        let bytes = STANDARD.decode(asset.data_base64.unwrap()).unwrap();
        renditions.push(bytes.clone());
        std::fs::write(f.0.join("actual.webp"), &bytes).unwrap();
        let value=run(json!({"actual":"actual.webp","reference":{"artifactId":id,"assetId":"picture:fills:0"},"threshold":0}),&f.policy(),&store).await.unwrap();
        assert_eq!(value["verdict"], verdict);
        assert_eq!(value["actual"]["mimeType"], "image/webp");
        assert_eq!(value["actual"]["sha256"], hash(&bytes));
        assert_eq!(
            value["comparisonScope"],
            "asset-pixels-against-native-node-export"
        );
        assert_eq!(
            value["reference"]["renderInfo"]["representation"],
            "native-node"
        );
    }
    let error = run(
        json!({"actual":"actual.webp","reference":{"path":"reference.png"}}),
        &f.policy(),
        &store,
    )
    .await
    .unwrap_err();
    assert!(error.message.contains("Page/card screenshots"));
    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = kind.to_vec();
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        if !data.len().is_multiple_of(2) {
            out.push(0);
        }
        out
    }
    let mut animation = b"WEBP".to_vec();
    animation.extend(chunk(b"VP8X", &[2, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
    animation.extend(chunk(b"ANIM", &[0, 0, 0, 0, 0, 0]));
    for encoded in renditions {
        let mut frame = vec![0; 16];
        frame[12] = 100;
        frame.extend_from_slice(&encoded[12..]);
        animation.extend(chunk(b"ANMF", &frame));
    }
    let mut encoded = b"RIFF".to_vec();
    encoded.extend_from_slice(&(animation.len() as u32).to_le_bytes());
    encoded.extend(animation);
    std::fs::write(f.0.join("animated.webp"), encoded).unwrap();
    let error=run(json!({"actual":"animated.webp","reference":{"artifactId":id,"assetId":"picture:fills:0"},"threshold":0}),&f.policy(),&store).await.unwrap_err();
    assert!(error.message.contains("Animated WebP"), "{}", error.message);
}

#[tokio::test]
async fn asset_comparison_rejects_missing_raw_unknown_and_corrupt_reference() {
    let f = fixture();
    for (representation, broken) in [
        ("original-fill", false),
        ("unknown", false),
        ("not-exported", false),
        ("native-node", true),
    ] {
        let (store, id) = asset_store(representation, broken).await;
        let i = json!({"actual":"actual.png","reference":{"artifactId":id,"assetId":"picture:fills:0"}});
        assert!(run(i, &f.policy(), &store).await.is_err());
        let missing =
            json!({"actual":"actual.png","reference":{"artifactId":id,"assetId":"missing"}});
        assert!(
            run(missing, &f.policy(), &store)
                .await
                .unwrap_err()
                .message
                .contains("not captured")
        );
    }
}
#[tokio::test]
async fn threshold_pass_and_fail() {
    let f = fixture();
    f.png("actual.png", BLACK);
    let mut i = input();
    i["threshold"] = json!(1.0);
    let r = run(i.clone(), &f.policy(), &ArtifactStore::default())
        .await
        .unwrap();
    assert_eq!(r["verdict"], "pass");
    i["threshold"] = json!(0.005);
    let r = run(i, &f.policy(), &ArtifactStore::default())
        .await
        .unwrap();
    assert_eq!(r["verdict"], "fail");
    assert_eq!(r["visual"]["changedRatio"], 1.0);
    assert!(r.get("diffPath").is_none());
}
#[tokio::test]
async fn a_diff_path_writes_the_diff_png_as_evidence() {
    let f = fixture();
    f.png("actual.png", BLACK);
    let mut i = input();
    i["threshold"] = json!(0.005);
    i["diffPath"] = json!(f.0.join("evidence/diff.png"));
    let r = run(i, &f.policy(), &ArtifactStore::default())
        .await
        .unwrap();
    assert_eq!(r["verdict"], "fail");
    let written = r["diffPath"].as_str().unwrap();
    assert!(written.ends_with("diff.png"), "{r}");
    assert!(f.0.join("evidence/diff.png").exists());
    assert!(r.get("diffPng").is_none(), "includeDiff was not asked for");
}
#[tokio::test]
async fn out_of_root_actual_rejected() {
    let f = fixture();
    let mut i = input();
    i["actual"] = json!(std::env::temp_dir().join("outside.png"));
    let e = run(i, &f.policy(), &ArtifactStore::default())
        .await
        .unwrap_err();
    assert!(e.message.contains("allowed root"), "{e}");
}
#[tokio::test]
async fn artifact_without_reference_has_recovery() {
    let f = fixture();
    let store = ArtifactStore::default();
    let a = store
        .insert(
            super::super::artifacts::ArtifactRequestKey::from_collection(
                &super::super::artifacts::w3_tests::request(),
            ),
            super::super::artifacts::w3_tests::payload(),
        )
        .await
        .unwrap();
    let mut i = input();
    i["reference"] = json!({"artifactId":a.artifact_id});
    let e = run(i, &f.policy(), &store).await.unwrap_err();
    assert!(
        e.message.contains("referencePng") && e.message.contains("devup_figma_export"),
        "{e}"
    );
}
#[tokio::test]
async fn diff_delivered_as_readable_resource() {
    let f = fixture();
    let store = ArtifactStore::default();
    let mut i = input();
    i["delivery"] = json!("resource");
    i["includeDiff"] = json!(true);
    let r = run(i, &f.policy(), &store).await.unwrap();
    assert!(r.get("diffPng").is_none());
    let m = &r["resources"][0];
    assert!(m["uri"].as_str().unwrap().starts_with("devup://artifact/"));
    let manifests = store.output_manifests().await;
    let bytes = store
        .read_output_chunk(&manifests[0].artifact_id, &manifests[0].output_id, 0)
        .await
        .unwrap();
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
}

#[tokio::test]
async fn incomplete_manifest_inconclusive() {
    let f = fixture();
    for field in [
        "renderer",
        "fontManifest",
        "viewport",
        "captureContext",
        "tsxResource",
        "referenceResource",
        "themePath",
        "assetDirectory",
        "os",
        "locale",
        "timezone",
        "devupUiVersion",
    ] {
        let mut i = input();
        i["environment"].as_object_mut().unwrap().remove(field);
        let r = run(i, &f.policy(), &ArtifactStore::default())
            .await
            .unwrap();
        assert_eq!(r["verdict"], "inconclusive", "{field}");
    }
}
#[tokio::test]
async fn declared_viewport_mismatch_inconclusive() {
    let f = fixture();
    let mut i = input();
    i["environment"]["viewport"]["width"] = json!(2);
    let r = run(i, &f.policy(), &ArtifactStore::default())
        .await
        .unwrap();
    assert_eq!(r["verdict"], "inconclusive");
    assert_eq!(r["visual"]["passed"], true);
}
include!("visual_compare_calibration_tests.rs");

#[tokio::test]
async fn reference_requires_exactly_one_selector() {
    for reference in [json!({}), json!({"path":"reference.png","artifactId":"id"})] {
        let mut i = input();
        i["reference"] = reference;
        assert!(serde_json::from_value::<VisualCompareInput>(i).is_err());
    }
}
#[tokio::test]
async fn traversal_and_ads_rejected() {
    let f = fixture();
    for path in [
        "../actual.png",
        "actual.png:stream",
        "Z:/actual.png",
        r"\\server\share\actual.png",
    ] {
        let mut i = input();
        i["actual"] = json!(path);
        let e = run(i, &f.policy(), &ArtifactStore::default())
            .await
            .unwrap_err();
        assert!(!e.message.contains("not implemented"));
    }
}

#[test]
fn bounded_read_preserves_png_bytes() {
    let f = fixture();
    let bytes = f
        .policy()
        .resolve("actual.png")
        .unwrap()
        .read_bounded(1024)
        .unwrap();
    assert_eq!(bytes, STANDARD.decode(WHITE).unwrap());
}
#[test]
fn bounded_read_refuses_oversize() {
    let f = fixture();
    let e = f
        .policy()
        .resolve("actual.png")
        .unwrap()
        .read_bounded(4)
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::DevupFigmaResponseTooLarge);
}
#[test]
fn symlink_escape_refused_before_read() {
    let f = fixture();
    let outside = fixture();
    #[cfg(windows)]
    // Like output_policy's directory-symlink test, this requires the host
    // to allow symlink creation. Windows reports missing privilege as 1314
    // (ErrorKind::Uncategorized on some toolchains), not PermissionDenied.
    if let Err(error) = std::os::windows::fs::symlink_dir(&outside.0, f.0.join("escape")) {
        if error.raw_os_error() == Some(1314) {
            eprintln!("symlink escape test unavailable: Windows symlink privilege is missing");
            return;
        }
        panic!("could not create test symlink: {error}");
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside.0, f.0.join("escape")).unwrap();
    assert!(
        f.policy()
            .resolve("escape/actual.png")
            .and_then(|t| t.read_bounded(1024))
            .is_err()
    );
    // A leaf link must also be rejected, including when swapped after resolve.
    let target = f.policy().resolve("late.png").unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(outside.0.join("actual.png"), f.0.join("late.png")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.0.join("actual.png"), f.0.join("late.png")).unwrap();
    assert!(target.read_bounded(1024).is_err());
}

#[tokio::test]
async fn artifact_reference_preserves_provenance() {
    let f = fixture();
    let store = ArtifactStore::default();
    let mut payload = super::super::artifacts::w3_tests::payload();
    payload.reference_png = Some(devup_mcp_figma::ReferencePng {
        mime_type: "image/png".into(),
        data_base64: WHITE.into(),
        byte_length: STANDARD.decode(WHITE).unwrap().len(),
        sha256: reference_hash(),
        note: None,
    });
    let artifact = store
        .insert(
            ArtifactRequestKey::from_collection(&super::super::artifacts::w3_tests::request()),
            payload,
        )
        .await
        .unwrap();
    let mut i = input();
    i["reference"] = json!({"artifactId":artifact.artifact_id});
    let result = run(i, &f.policy(), &store).await.unwrap();
    assert_eq!(result["verdict"], "pass");
    assert_eq!(result["reference"]["artifactId"], artifact.artifact_id);
    assert_eq!(result["reference"]["referenceSha256"], reference_hash());
    assert_eq!(result["visual"]["maxChangedRatio"], 0.005);
}
/// The server holds the reference it captured, so it pins it itself; a
/// reference path is the consumer's file and still needs their pin.
#[tokio::test]
async fn a_held_reference_is_pinned_by_the_server() {
    let f = fixture();
    let store = ArtifactStore::default();
    let mut payload = super::super::artifacts::w3_tests::payload();
    payload.reference_png = Some(devup_mcp_figma::ReferencePng {
        mime_type: "image/png".into(),
        data_base64: WHITE.into(),
        byte_length: STANDARD.decode(WHITE).unwrap().len(),
        sha256: reference_hash(),
        note: None,
    });
    let artifact = store
        .insert(
            ArtifactRequestKey::from_collection(&super::super::artifacts::w3_tests::request()),
            payload,
        )
        .await
        .unwrap();
    let mut i = input();
    i["reference"] = json!({"artifactId":artifact.artifact_id});
    i["environment"]
        .as_object_mut()
        .unwrap()
        .remove("referenceResource");
    let result = run(i, &f.policy(), &store).await.unwrap();
    assert_eq!(result["verdict"], "pass", "{result}");
    assert_eq!(
        result["environment"]["filledByServer"],
        json!(["referenceResource"])
    );

    let mut by_path = input();
    by_path["environment"]
        .as_object_mut()
        .unwrap()
        .remove("referenceResource");
    let result = run(by_path, &f.policy(), &store).await.unwrap();
    assert_eq!(result["verdict"], "inconclusive");
    assert!(result["environment"].get("filledByServer").is_none());
}

#[tokio::test]
async fn renderer_and_reference_hash_mismatch_override_pass() {
    let f = fixture();
    let mut i = input();
    i["environment"]["expectedRenderer"] =
        json!({"name":"playwright-chromium","version":"different"});
    let r = run(i, &f.policy(), &ArtifactStore::default())
        .await
        .unwrap();
    assert_eq!(r["verdict"], "inconclusive");
    assert_eq!(r["visual"]["passed"], true);
    let mut i = input();
    i["environment"]["referenceResource"]["sha256"] = json!("c".repeat(64));
    let r = run(i, &f.policy(), &ArtifactStore::default())
        .await
        .unwrap();
    assert_eq!(r["verdict"], "inconclusive");
}
#[tokio::test]
async fn inline_diff_and_omitted_diff_follow_request() {
    let f = fixture();
    let r = run(input(), &f.policy(), &ArtifactStore::default())
        .await
        .unwrap();
    assert!(r.get("diffPng").is_none());
    let mut i = input();
    i["includeDiff"] = json!(true);
    let r = run(i, &f.policy(), &ArtifactStore::default())
        .await
        .unwrap();
    assert!(
        STANDARD
            .decode(r["diffPng"]["dataBase64"].as_str().unwrap())
            .unwrap()
            .starts_with(b"\x89PNG")
    );
    assert!(r.get("resources").is_none());
}
