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
    Asset(AssetReference),
    Artifact(ArtifactReference),
    Path(PathReference),
}
/// A validated native Figma node PNG captured by devup_figma_export. Raw fill
/// captures cannot serve as a reference for their own canvas appearance.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct AssetReference {
    pub artifact_id: String,
    pub asset_id: String,
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
    pub capture_context: Option<CaptureContext>,
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
/// Actual browser measurements, consumer-attested rather than inferred from PNGs.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CaptureContext {
    pub inner_width: u32,
    pub inner_height: u32,
    pub device_pixel_ratio: f64,
    pub visual_viewport_scale: f64,
    pub root_css_zoom: f64,
    pub body_css_zoom: f64,
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
        match &e.viewport {
            Some(v) if v.width > 0 && v.height > 0
                && v.device_scale_factor.is_finite() && v.device_scale_factor > 0.0 => {
                if v.width != dimensions[0] {
                    issues.push("CSS viewport width differs from PNG width; capture scope and logical-to-pixel calibration are not established");
                }
                if v.device_scale_factor != 1.0 {
                    issues.push("Non-unit DPR does not prove PNG capture scale. This manifest has no independent logical capture extent/export-scale calibration; strict pixels remain inconclusive without that evidence");
                }
            }
            _ => issues.push("positive CSS viewport width/height and finite positive deviceScaleFactor are required"),
        }
        match (&e.capture_context, &e.viewport) {
            (Some(c), Some(v)) if c.inner_width == v.width && c.inner_height == v.height
                && c.device_pixel_ratio.is_finite() && c.device_pixel_ratio > 0.0
                && c.device_pixel_ratio == v.device_scale_factor
                && c.visual_viewport_scale == 1.0 && c.root_css_zoom == 1.0 && c.body_css_zoom == 1.0 => {}
            _ => issues.push("Actual inner viewport, DPR, visualViewport.scale and root/body CSS zoom measurements are missing, inconsistent or uncalibrated; strict pixels remain inconclusive"),
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
    mut input: VisualCompareInput,
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
        VisualReference::Asset(r) => {
            if r.asset_id.trim().is_empty() {
                return Err(invalid(
                    "assetId must identify a captured native PNG asset.",
                ));
            }
            let artifact = store.get_for_export(&r.artifact_id).await?;
            let asset = artifact.payload.assets.iter().find(|a| a.asset_id == r.asset_id)
                .ok_or_else(|| invalid("Reference asset is not captured in this artifact; export it with renderMode node and format png first."))?;
            let info = asset.render_info.as_ref();
            if asset.status != devup_mcp_figma::AssetStatus::Exported
                || asset.format != Some(devup_mcp_figma::AssetFormat::Png)
                || asset.mime_type.as_deref() != Some("image/png")
                || info.is_none_or(|i| i["representation"] != "native-node")
            {
                return Err(invalid(
                    "Asset reference requires a successful native-node PNG capture; raw fills, fallback geometry, failed or unknown captures are not canvas references.",
                ));
            }
            let bytes = STANDARD
                .decode(asset.data_base64.as_deref().unwrap_or_default())
                .map_err(|_| invalid("Reference asset base64 is invalid."))?;
            if bytes.is_empty()
                || bytes.len() > 16 * 1024 * 1024
                || asset.byte_length != Some(bytes.len())
                || asset.sha256.as_deref() != Some(hash(&bytes).as_str())
            {
                return Err(invalid(
                    "Reference asset length or hash mismatch; reacquire it.",
                ));
            }
            let provenance = json!({"kind":"asset","artifactId":r.artifact_id,"assetId":r.asset_id,"nodeId":asset.node_id,
                "referenceSha256":asset.sha256,"target":artifact.payload.target,"sourceVersion":artifact.payload.source_version,
                "renderMode":asset.render_mode,"renderInfo":asset.render_info,"scope":"native-node-export; ancestor canvas compositing unverified"});
            (bytes, provenance, Some(artifact))
        }
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
    let actual_hash = hash(&actual);
    let asset_comparison = matches!(&input.reference, VisualReference::Asset(_));
    let webp_actual = image::guess_format(&actual).ok() == Some(image::ImageFormat::WebP);
    if webp_actual && !asset_comparison {
        return Err(invalid(
            "WebP actuals require a native PNG asset reference (artifactId plus assetId). Page/card screenshots and their environment checks continue to require PNG.",
        ));
    }
    let diff_target = input
        .diff_path
        .as_deref()
        .map(|path| policy.resolve(path))
        .transpose()?;
    let include_diff = input.include_diff || diff_target.is_some();
    let reference_hash = hash(&reference);
    let (report, diff) = tokio::task::spawn_blocking(move || {
        let actual = if webp_actual {
            use image::ImageDecoder;
            let mut limits = image::Limits::default(); limits.max_image_width=Some(8192); limits.max_image_height=Some(8192); limits.max_alloc=Some(64*1024*1024);
            let mut decoder=image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(actual)).map_err(|_| "Invalid WebP actual.")?;
            decoder.set_limits(limits).map_err(|_| "WebP actual exceeds bounded decode limits.")?;
            if decoder.has_animation() {return Err("Animated WebP actuals cannot establish a static asset comparison; provide a static WebP or an explicit PNG screenshot.");}
            let (w,h)=decoder.dimensions();
            if u64::from(w)*u64::from(h)>devup_mcp_figma::MAX_WEBP_PIXELS { return Err("WebP actual exceeds 4194304 decoded pixels."); }
            let image=image::DynamicImage::from_decoder(decoder).map_err(|_| "Malformed WebP actual.")?;
            let mut png=std::io::Cursor::new(Vec::new());image.write_to(&mut png,image::ImageFormat::Png).map_err(|_| "WebP comparison pixel preparation failed.")?;png.into_inner()
        } else {actual};
        compare_png_bytes(&reference,&actual,&options,include_diff).map_err(|_| "Invalid PNG or threshold; PNG limits are 16 MiB compressed, 8192 pixels per axis, and 64 MiB decoded.")
    })
        .await.map_err(|_| invalid("PNG comparison could not complete."))?
        .map_err(invalid)?;
    if report.status == VisualStatus::InvalidDimensions {
        if asset_comparison {
            return Err(DevupError::with_details(
                ErrorCode::DevupCodegenFailed,
                "Native asset dimensions differ from the captured node export; compatible capture extent and scale are required for asset pixel comparison.",
                false,
                json!({"comparisonScope":"asset-pixels-against-native-node-export",
                    "referenceDimensions":report.reference_dimensions,"actualDimensions":report.actual_dimensions,
                    "reference":provenance}),
            ));
        }
        return Ok(json!({
            "verdict":"inconclusive", "visual":report,
            "environment":{"status":"environment-invalid","issues":["Incompatible PNG dimensions; capture scope and logical-to-pixel calibration are not established"],"declaration":input.environment},
            "reference":provenance,
            "actual":{"path":actual_target.display_path(),"sha256":actual_hash},
            "note":"Compare logical geometry separately. Do not resize, pad or crop either image to force a match; raster dimensions alone do not establish a layout failure.",
        }));
    }
    // A reference this server captured from Figma and still holds is pinned
    // by what the server read, so a consumer need not restate its hash.
    let mut filled = Vec::new();
    if let (VisualReference::Artifact(reference), Some(declared)) =
        (&input.reference, input.environment.as_mut())
        && declared.reference_resource.is_none()
    {
        declared.reference_resource = Some(ResourcePin {
            uri: format!("devup://artifact/{}", reference.artifact_id),
            sha256: reference_hash.clone(),
        });
        filled.push("referenceResource");
    }
    let mut environment = if asset_comparison {
        json!({"status":"valid","scope":"asset-pixels-only","issues":[],"trust":"server-validated native PNG reference and allowlisted actual raster; no browser environment required"})
    } else {
        assess_environment(&input, report.reference_dimensions, &reference_hash)
    };
    if !filled.is_empty() {
        environment["filledByServer"] = json!(filled);
    }
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
    result["comparisonScope"] = json!(if asset_comparison {
        "asset-pixels-against-native-node-export"
    } else {
        "consumer-render-against-reference"
    });
    result["actual"] = json!({"path":actual_target.display_path(),"sha256":actual_hash});
    result["actual"]["mimeType"] = json!(if webp_actual {
        "image/webp"
    } else {
        "image/png"
    });
    result["actual"]["pixelMetricsInput"] = json!(if webp_actual {
        "decoded-WebP; temporary PNG encoding for comparison"
    } else {
        "PNG"
    });
    if asset_comparison {
        result["canvasFidelity"] = json!("unverified-outside-reference-node-subtree");
    }
    if let (Some(target), Some(bytes)) = (diff_target, diff.as_ref()) {
        let display = target.display_path().display().to_string();
        let mut transaction = super::output::OutputTransaction::new();
        transaction.stage("diff", target, bytes)?;
        transaction.commit()?;
        result["diffPath"] = json!(display);
    }
    let diff = diff.filter(|_| input.include_diff);
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
#[path = "visual_compare_tests.rs"]
mod tests;
