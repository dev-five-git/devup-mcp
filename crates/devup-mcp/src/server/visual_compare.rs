//! Pure PNG comparison and consumer-attested renderer environment assessment.
//! PNG bytes stay in memory and use the existing bounded artifact resource store.
use super::{
    artifacts::{ArtifactRequestKey, ArtifactStore},
    delivery::{ProjectedOutput, choose_delivery_for_result},
    output::OutputPolicy,
    tools::VisualCompareInput,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp_figma::{
    CollectedPayload, CollectionRequest, CollectionScope, CollectionStats, DevupError, ErrorCode,
    FigmaTarget, PayloadCompleteness, Snapshot,
};
use devup_mcp_visual::{CompareOptions, VisualStatus, compare_png_bytes};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum VisualReference {
    Artifact(ArtifactReference),
    Path(PathReference),
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct ArtifactReference {
    pub artifact_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct PathReference {
    pub path: String,
}

/// Consumer declarations are echoed, never fetched or executed. Optional fields
/// allow an incomplete run manifest to produce an explicit inconclusive result.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VisualEnvironment {
    pub schema_version: Option<u32>,
    pub tsx_resource: Option<ResourcePin>,
    pub reference_resource: Option<ResourcePin>,
    pub viewport: Option<Viewport>,
    pub theme_path: Option<String>,
    pub asset_directory: Option<String>,
    pub font_manifest: Option<Vec<FontPin>>,
    pub renderer: Option<RendererPin>,
    /// Optional independently expected renderer pin; disagreement is invalid.
    pub expected_renderer: Option<RendererPin>,
    pub devup_ui_version: Option<String>,
    pub actual_png: Option<String>,
    /// Consumer must set environment-invalid for missing fonts/assets, failed
    /// readiness, console/runtime errors, or any mismatch with its pinned CI.
    #[schemars(extend("enum" = ["valid", "environment-invalid"]))]
    pub environment_status: Option<String>,
    pub os: Option<String>,
    pub locale: Option<String>,
    pub timezone: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ResourcePin {
    pub uri: String,
    pub sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    pub device_scale_factor: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FontPin {
    pub family: String,
    pub sha256: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RendererPin {
    pub name: String,
    pub version: String,
}

fn invalid(message: &str) -> DevupError {
    DevupError::new(ErrorCode::DevupCodegenFailed, message, false)
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn pinned(value: &str) -> bool {
    !value.trim().is_empty()
}
fn sha(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn assess_environment(
    input: &VisualCompareInput,
    dimensions: [u32; 2],
    reference_hash: &str,
) -> Value {
    let mut issues = Vec::new();
    if let Some(e) = &input.environment {
        if e.schema_version != Some(1) {
            issues.push("schemaVersion must be 1");
        }
        if e.environment_status.as_deref() != Some("valid") {
            issues.push("consumer did not attest a valid environment");
        }
        for (value, issue) in [
            (&e.theme_path, "themePath missing"),
            (&e.asset_directory, "assetDirectory missing"),
            (&e.devup_ui_version, "devupUiVersion missing"),
            (&e.os, "OS pin missing"),
            (&e.locale, "locale missing"),
            (&e.timezone, "timezone missing"),
        ] {
            if !value.as_deref().is_some_and(pinned) {
                issues.push(issue);
            }
        }
        if e.actual_png.as_deref() != Some(input.actual.as_str()) {
            issues.push("actualPng must identify the supplied actual path");
        }
        if !e
            .viewport
            .as_ref()
            .is_some_and(|v| [v.width, v.height] == dimensions && v.device_scale_factor == 1.0)
        {
            issues.push("viewport or deviceScaleFactor missing or mismatched; capture reference dimensions at scale 1");
        }
        if !e
            .renderer
            .as_ref()
            .is_some_and(|r| pinned(&r.name) && pinned(&r.version))
        {
            issues.push("renderer name/version pin missing");
        }
        if e.expected_renderer.is_some() && e.expected_renderer != e.renderer {
            issues.push("renderer pin mismatch");
        }
        if !e
            .font_manifest
            .as_ref()
            .is_some_and(|fonts| fonts.iter().all(|f| pinned(&f.family) && sha(&f.sha256)))
        {
            issues.push("fontManifest missing or font hashes invalid");
        }
        if !e
            .tsx_resource
            .as_ref()
            .is_some_and(|r| pinned(&r.uri) && sha(&r.sha256))
        {
            issues.push("tsxResource URI/hash missing or invalid");
        }
        if !e
            .reference_resource
            .as_ref()
            .is_some_and(|r| pinned(&r.uri) && r.sha256.eq_ignore_ascii_case(reference_hash))
        {
            issues.push("referenceResource URI/hash missing or mismatched");
        }
    } else {
        issues.push("consumer environment manifest missing");
    }
    json!({"status":if issues.is_empty(){"valid"}else{"environment-invalid"},"issues":issues,
        "declaration":input.environment,"trust":"consumer-attested; PNGs cannot verify fonts, assets, readiness, runtime errors, or CI pins"})
}

pub(super) async fn compare(
    input: VisualCompareInput,
    policy: &OutputPolicy,
    store: &ArtifactStore,
) -> Result<Value, DevupError> {
    let delivery = input.delivery.parse()?;
    let options = CompareOptions {
        max_changed_ratio: input.threshold.unwrap_or(0.005),
        ..CompareOptions::default()
    };
    if !(0.0..=1.0).contains(&options.max_changed_ratio) {
        return Err(invalid("threshold must be between 0 and 1 inclusive."));
    }
    // Resolve every supplied path before any PNG read.
    let actual_target = policy.resolve(&input.actual)?;
    let reference_target = match &input.reference {
        VisualReference::Path(r) => Some(policy.resolve(&r.path)?),
        _ => None,
    };
    let (reference, provenance, artifact) = match &input.reference {
        VisualReference::Artifact(r) => {
            let artifact = store.get_for_export(&r.artifact_id).await?;
            let png = artifact.payload.reference_png.as_ref().ok_or_else(|| DevupError::with_details(
                ErrorCode::DevupFigmaHandoffInvalid,
                "This artifact holds no referencePng. Call devup_figma_export with the original url and outputs containing referencePng, then retry with the returned artifactId.", false,
                json!({"recoveryState":"recoverable","nextAction":{"tool":"devup_figma_export","outputs":["referencePng"]}})))?;
            if png.byte_length > 16 * 1024 * 1024
                || png.data_base64.len() > (16 * 1024 * 1024_usize).div_ceil(3) * 4
            {
                return Err(invalid(
                    "Cached referencePng exceeds the compressed byte limit.",
                ));
            }
            let bytes = STANDARD.decode(&png.data_base64).map_err(|_| {
                invalid("Cached referencePng is invalid; reacquire it with devup_figma_export.")
            })?;
            if bytes.len() != png.byte_length || hash(&bytes) != png.sha256 {
                return Err(invalid(
                    "Cached referencePng hash mismatch; reacquire it with devup_figma_export.",
                ));
            }
            let provenance = json!({"kind":"artifact","artifactId":artifact.artifact_id,"referenceSha256":png.sha256,
                "target":artifact.payload.target,"sourceVersion":artifact.payload.source_version,"expiresAtEpochSeconds":artifact.expires_at_epoch_seconds});
            (bytes, provenance, Some(artifact))
        }
        VisualReference::Path(_) => {
            let target = reference_target.as_ref().expect("resolved reference");
            let bytes = target.read_bounded(16 * 1024 * 1024)?;
            let provenance =
                json!({"kind":"path","path":target.display_path(),"referenceSha256":hash(&bytes)});
            (bytes, provenance, None)
        }
    };
    let actual = actual_target.read_bounded(16 * 1024 * 1024)?;
    let include_diff = input.include_diff;
    let reference_hash = hash(&reference);
    let (report, diff) = tokio::task::spawn_blocking(move || compare_png_bytes(&reference,&actual,&options,include_diff))
        .await.map_err(|_| invalid("PNG comparison could not complete."))?
        .map_err(|_| invalid("Invalid PNG or threshold; PNG limits are 16 MiB compressed, 8192 pixels per axis, and 64 MiB decoded."))?;
    if report.status == VisualStatus::InvalidDimensions {
        return Err(DevupError::with_details(
            ErrorCode::DevupCodegenFailed,
            "PNG dimensions mismatch; render actual.png at reference dimensions without scaling.",
            false,
            json!({"verdict":"inconclusive","environment":{"status":"environment-invalid","issues":["image dimensions mismatch"]},
                "referenceDimensions":report.reference_dimensions,"actualDimensions":report.actual_dimensions,"reference":provenance}),
        ));
    }
    let environment = assess_environment(&input, report.reference_dimensions, &reference_hash);
    let passed = report.passed();
    let verdict = if environment["status"] != "valid" {
        "inconclusive"
    } else if passed {
        "pass"
    } else {
        "fail"
    };
    let mut visual =
        serde_json::to_value(report).map_err(|_| invalid("Cannot serialize visual metrics."))?;
    visual["passed"] = json!(passed);
    let mut result =
        json!({"verdict":verdict,"visual":visual,"environment":environment,"reference":provenance});
    if let Some(bytes) = diff {
        result["diffPng"] = json!({"mimeType":"image/png","dataBase64":STANDARD.encode(&bytes)});
        let outputs = vec![ProjectedOutput::binary("diff.png", "image/png", bytes)];
        if !choose_delivery_for_result(delivery, &result, &outputs)?.inline {
            // Paths also need a bounded, expiring resource owner. Its random
            // identity and content-free payload never contain PNGs in cache keys.
            let artifact = match artifact {
                Some(a) => a,
                None => {
                    let target = FigmaTarget {
                        file_key: format!("visual-{:032x}", rand::random::<u128>()),
                        node_id: None,
                        branch_key: None,
                    };
                    let request = CollectionRequest::new(target.clone(), CollectionScope::Node);
                    let payload = CollectedPayload {
                        target: target.clone(),
                        scope: CollectionScope::Node,
                        metadata: json!({"kind":"visual-comparison"}),
                        snapshot: Snapshot {
                            file_key: target.file_key,
                            version: None,
                            roots: vec![],
                            nodes: BTreeMap::new(),
                            diagnostics: vec![],
                        },
                        variables: None,
                        styles: None,
                        completeness: PayloadCompleteness::ResolvedValuesOnly,
                        source_version: None,
                        stats: CollectionStats::default(),
                        assets: vec![],
                        reference_png: None,
                        failures: vec![],
                    };
                    store
                        .insert(ArtifactRequestKey::from_collection(&request), payload)
                        .await?
                }
            };
            let projection_key = format!("visual-{:032x}", rand::random::<u128>());
            let manifests = store
                .attach_outputs(&artifact.artifact_id, &projection_key, outputs)
                .await?;
            result.as_object_mut().expect("object").remove("diffPng");
            result["resources"]=json!(manifests.iter().map(|m|json!({"uri":m.manifest_uri,"name":m.name,"mimeType":m.mime_type,"size":m.raw_bytes,"contentHash":m.sha256,"expiresAt":super::format_epoch_rfc3339(m.expires_at_epoch_seconds)})).collect::<Vec<_>>());
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn run(
        input: Value,
        policy: &OutputPolicy,
        store: &ArtifactStore,
    ) -> Result<Value, DevupError> {
        let input = serde_json::from_value(input)
            .map_err(|_| invalid("Invalid visual comparison input."))?;
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
        json!({"schemaVersion":1,"tsxResource":{"uri":"devup://artifact/test/manifest","sha256":"a".repeat(64)},"referenceResource":{"uri":"devup://artifact/test/manifest","sha256":reference_hash()},"viewport":{"width":1,"height":1,"deviceScaleFactor":1},"themePath":"devup.json","assetDirectory":"public/assets","fontManifest":[{"family":"Test","sha256":"b".repeat(64)}],"renderer":{"name":"playwright-chromium","version":"1"},"devupUiVersion":"1","actualPng":"actual.png","environmentStatus":"valid","os":"test","locale":"en-US","timezone":"UTC"})
    }
    fn input() -> Value {
        json!({"actual":"actual.png","reference":{"path":"reference.png"},"environment":environment()})
    }
    const WHITE: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4////fwAJ+wP9KobjigAAAABJRU5ErkJggg==";
    const BLACK: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNgYGD4DwABBAEAX+XDSwAAAABJRU5ErkJggg==";
    const WIDE: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAC0lEQVR4nGP4DwUAI+UH+Yo0eLMAAAAASUVORK5CYII=";
    fn fixture() -> Fixture {
        let f = Fixture::new();
        f.png("actual.png", WHITE);
        f.png("reference.png", WHITE);
        f
    }
    #[tokio::test]
    async fn dimension_mismatch_refused() {
        let f = fixture();
        f.png("actual.png", WIDE);
        let e = run(input(), &f.policy(), &ArtifactStore::default())
            .await
            .unwrap_err();
        assert!(e.message.contains("dimensions"), "{e}");
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
        std::os::windows::fs::symlink_dir(&outside.0, f.0.join("escape")).unwrap();
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
        std::os::windows::fs::symlink_file(outside.0.join("actual.png"), f.0.join("late.png"))
            .unwrap();
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
}
