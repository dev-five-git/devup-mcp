//! Export observations are evidence, never a claim of canvas fidelity.
use std::io::Cursor;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::{AssetFormat, AssetManifestEntry, AssetRenderMode, Snapshot};

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn size(width: Option<f64>, height: Option<f64>) -> Value {
    match (width, height) {
        (Some(w), Some(h)) if w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 => {
            json!({"width":w,"height":h,"aspectRatio":w/h})
        }
        _ => Value::Null,
    }
}

fn filename_part(name: &str) -> String {
    let mut slug = String::new();
    for c in name.nfc() {
        if c.is_alphanumeric() {
            slug.extend(c.to_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
        if slug.len() >= 24 {
            break;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "layer".to_owned()
    } else {
        slug.to_owned()
    }
}

fn asset_excluded(snapshot: &Snapshot, node_id: &str, field: &str) -> bool {
    let Some(node) = snapshot.nodes.get(node_id) else {
        return false;
    };
    crate::asset_exclusion_reason(node).is_some()
        || field
            .strip_prefix("fills/")
            .and_then(|s| s.parse::<usize>().ok())
            .and_then(|i| node.typed_view().value("fills")?.get(i))
            .is_some_and(|p| {
                p["visible"] == false || p["opacity"].as_f64().is_some_and(|n| n <= 0.0)
            })
}

fn effective_scale(pixels: &Value, node: &Value) -> Value {
    match (
        pixels["width"].as_f64(),
        pixels["height"].as_f64(),
        node["width"].as_f64(),
        node["height"].as_f64(),
    ) {
        (Some(w), Some(h), Some(nw), Some(nh)) if w > 0.0 && h > 0.0 && nw > 0.0 && nh > 0.0 => {
            json!({"x":w/nw,"y":h/nh,"minimum":(w/nw).min(h/nh)})
        }
        _ => Value::Null,
    }
}

pub(crate) fn measured_info(
    mode: AssetRenderMode,
    format: AssetFormat,
    scale: f64,
    observed: Option<Value>,
    bytes: Option<&[u8]>,
) -> Value {
    let mut info = observed
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    info["requestedScale"] = if scale.fract() == 0.0 {
        json!(scale as u64)
    } else {
        json!(scale)
    };
    let effective = if info["representation"]
        .as_str()
        .is_some_and(|s| s.starts_with("original-fill"))
    {
        AssetRenderMode::Fill
    } else {
        mode
    };
    info["scaleSemantics"] = json!(match (effective, format) {
        (AssetRenderMode::Fill | AssetRenderMode::OriginalSource, AssetFormat::Webp) =>
            "intrinsic-source-transcoded-no-resize",
        (AssetRenderMode::Fill | AssetRenderMode::OriginalSource, _) =>
            "intrinsic-original-bytes-no-resize",
        (_, AssetFormat::Png | AssetFormat::Jpg | AssetFormat::Webp) =>
            "native-node-SCALE-constraint",
        _ => "not-applied-to-vector-document",
    });
    if info.get("representation").is_none() {
        info["representation"] = json!(if mode.is_original() {
            "original-fill"
        } else {
            "unknown"
        });
    }
    info["outputSize"] = bytes
        .filter(|_| {
            matches!(
                format,
                AssetFormat::Png | AssetFormat::Jpg | AssetFormat::Webp
            )
        })
        .and_then(|bytes| {
            if format == AssetFormat::Webp
                && image::guess_format(bytes).ok() == Some(image::ImageFormat::Png)
                && crate::webp::validate_png_input(bytes).is_err()
            {
                return None;
            }
            let mut reader = image::ImageReader::new(Cursor::new(bytes))
                .with_guessed_format()
                .ok()?;
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(crate::MAX_WEBP_DIMENSION);
            limits.max_image_height = Some(crate::MAX_WEBP_DIMENSION);
            limits.max_alloc = Some(128 * 1024 * 1024);
            reader.limits(limits);
            reader.into_dimensions().ok()
        })
        .map(|(w, h)| size(Some(f64::from(w)), Some(f64::from(h))))
        .unwrap_or(Value::Null);
    info["dimensionEvidence"] = json!(if info["outputSize"].is_null() {
        "unknown-or-non-raster"
    } else {
        "encoded-byte-header"
    });
    info["byteEvidence"] = bytes
        .map(|b| json!({"sha256":hash(b),"byteLength":b.len(),"kind":"validated-export-bytes"}))
        .unwrap_or(Value::Null);
    compare(&mut info);
    info
}

fn compare(info: &mut Value) {
    let node_ratio = info["nodeSize"]["aspectRatio"].as_f64();
    let output_ratio = info["outputSize"]["aspectRatio"].as_f64();
    let comparison = match (node_ratio, output_ratio) {
        (Some(n), Some(o)) if n > 0.0 && o > 0.0 => {
            // A pixel of raster rounding is expected, particularly for tiny icons.
            let w = info["outputSize"]["width"].as_f64().unwrap_or(0.0);
            let h = info["outputSize"]["height"].as_f64().unwrap_or(0.0);
            if (w - h * n).abs() > 1.0 && (o / n - 1.0).abs() > 0.01 {
                "mismatch"
            } else {
                "match"
            }
        }
        _ => "unknown",
    };
    info["aspectRatioComparison"] = json!(comparison);
    info["aspectRatioNote"] = json!(
        "A mismatch can reflect crop, rotation or effect extents; a matching ratio does not establish visual fidelity."
    );
    let warnings = info
        .as_object_mut()
        .unwrap()
        .entry("warnings")
        .or_insert_with(|| json!([]));
    if !warnings.is_array() {
        *warnings = json!([]);
    }
    let warnings = warnings.as_array_mut().unwrap();
    warnings.retain(|v| {
        !matches!(
            v.as_str(),
            Some(
                "aspect-ratio-mismatch"
                    | "aspect-ratio-unknown"
                    | "blur-baked-in"
                    | "shadow-baked-in"
                    | "output-dimensions-differ-from-layout"
                    | "crop-transform-not-applied"
                    | "extra-fills-not-applied"
                    | "effects-not-applied"
            )
        )
    });
    if comparison != "match" {
        warnings.push(json!(if comparison == "mismatch" {
            "aspect-ratio-mismatch"
        } else {
            "aspect-ratio-unknown"
        }));
    }
    if info["scaleSemantics"] == "intrinsic-original-bytes-no-resize"
        || info["scaleSemantics"] == "intrinsic-source-transcoded-no-resize"
    {
        let warnings = info["warnings"].as_array_mut().unwrap();
        if !warnings.contains(&json!("original-fill-properties-unapplied")) {
            warnings.push(json!("original-fill-properties-unapplied"));
        }
        if info["unappliedProperties"].is_null() {
            info["unappliedProperties"] = json!([
                "scaleMode",
                "imageTransform",
                "rotation",
                "filters",
                "paintOpacity",
                "nodeGeometry",
                "nodeTransforms",
                "nodeEffectsIncludingBlur",
                "nodeAndAncestorMasks",
                "ancestorClipsAndEffects"
            ]);
        }
    }
    let representation = info["representation"]
        .as_str()
        .unwrap_or("unknown")
        .to_owned();
    info["renderedAs"] = json!(match representation.as_str() {
        "original-fill" => "raw-fill",
        "original-fill-transcoded" => "transcoded-fill",
        "native-node" => "node-render",
        "native-node-transcoded" => "transcoded-node-render",
        "native-visible" => "visible-scoped-capture",
        "native-visible-transcoded" => "transcoded-visible-scoped-capture",
        "geometry-fallback" => "geometry-fallback",
        "not-exported" => "not-exported",
        _ => "unknown",
    });
    for (key, source) in [
        ("scaleMode", "scaleMode"),
        ("imageTransform", "imageTransform"),
        ("effects", "effects"),
    ] {
        if info[key].is_null() {
            info[key] = info["sourceProperties"][source].clone();
        }
    }
    info["sourcePixelSize"] = info["sourceSize"].clone();
    info["nodeBox"] = info["nodeSize"].clone();
    if representation.starts_with("original-fill")
        && info["sourcePixelSize"].is_null()
        && info["resize"]["applied"] != true
    {
        info["sourcePixelSize"] = info["outputSize"].clone();
        info["sourceSize"] = info["outputSize"].clone();
        info["sourceSizeEvidence"] = info["dimensionEvidence"].clone();
    }
    let source_ratio = info["sourcePixelSize"]["aspectRatio"].as_f64();
    let node_ratio = info["nodeBox"]["aspectRatio"].as_f64();
    info["visibleSourceRatio"] = match (
        info["scaleMode"]
            .as_str()
            .filter(|_| info["sourceProperties"]["rotation"].as_f64() == Some(0.0)),
        source_ratio,
        node_ratio,
    ) {
        (Some("FIT"), Some(_), Some(_)) => json!({"width":1.0,"height":1.0}),
        (Some("FILL"), Some(source), Some(node)) if source > 0.0 && node > 0.0 => {
            json!({"width":(node/source).min(1.0),"height":(source/node).min(1.0)})
        }
        _ => Value::Null,
    };
    info["visibleSourceRatioEvidence"] = json!(if info["visibleSourceRatio"].is_null() {
        "unknown; crop/rotation/mask coverage requires geometry; transform-coefficients-are-not-visible-percentages"
    } else {
        "centered-FILL-or-FIT-source-and-layout-aspect-ratios; node-shape-and-ancestor-coverage-excluded"
    });
    info["sourceEffectiveScale"] = effective_scale(&info["sourcePixelSize"], &info["nodeBox"]);
    info["outputEffectiveScale"] = effective_scale(
        &info["outputSize"],
        if representation.starts_with("native-visible") {
            &info["captureBox"]
        } else {
            &info["nodeBox"]
        },
    );
    info["effectiveScale"] = if representation.starts_with("original-fill") {
        info["sourceEffectiveScale"].clone()
    } else {
        info["outputEffectiveScale"].clone()
    };
    info["recommendedBitmapScale"] = json!({"default":2,"minimum":1.5,"maximum":3});
    if info["extraFills"].is_null() {
        info["extraFills"] = Value::Null;
    }
    if info["instanceOverride"].is_null() {
        info["instanceOverride"] = json!("unknown");
        info["instanceOverrideEvidence"] = json!("no-component-paint-comparison");
    }
    let measured = info["byteEvidence"].is_object() && representation != "not-exported";
    let native = matches!(
        representation.as_str(),
        "native-node" | "native-node-transcoded" | "native-visible" | "native-visible-transcoded"
    );
    info["effectsBakedIntoAsset"] = if measured && !info["outputSize"].is_null() {
        if native {
            json!(true)
        } else if representation.starts_with("original-fill") {
            json!(false)
        } else {
            Value::Null
        }
    } else if representation == "not-exported" {
        json!(false)
    } else {
        Value::Null
    };
    info["effectsBaked"] = info["effectsBakedIntoAsset"].clone();
    info["effectsAppliedBy"] = if !measured {
        json!("unknown")
    } else if native {
        json!("asset")
    } else if representation.starts_with("original-fill") {
        json!("css")
    } else {
        json!("unknown")
    };
    info["cropApplied"] = if representation.starts_with("original-fill") && measured {
        json!(false)
    } else if measured && native {
        if info["scaleMode"] == "CROP" {
            json!(true)
        } else if let (Some(w), Some(h)) = (
            info["visibleSourceRatio"]["width"].as_f64(),
            info["visibleSourceRatio"]["height"].as_f64(),
        ) {
            json!(w < 1.0 || h < 1.0)
        } else {
            Value::Null
        }
    } else {
        Value::Null
    };
    let mut additional = Vec::new();
    if info["cropApplied"] == true {
        additional.push("visible-crop-only");
    }
    if native && info["requestedScale"].as_f64().is_some_and(|s| s > 3.0) {
        additional.push("bitmap-scale-above-recommended-range");
    }
    if native && info["requestedScale"].as_f64().is_some_and(|s| s <= 1.0) {
        additional.push("low-resolution-bitmap");
    }
    if representation.starts_with("original-fill")
        && info["sourceEffectiveScale"]["minimum"]
            .as_f64()
            .is_some_and(|s| s < 1.5)
    {
        additional.push("source-resolution-below-1.5x-display");
    }
    if representation.starts_with("original-fill")
        && info["outputEffectiveScale"]["minimum"]
            .as_f64()
            .is_some_and(|s| s < 1.5)
    {
        additional.push("output-resolution-below-1.5x-display");
    }
    for warning in additional {
        if !info["warnings"]
            .as_array()
            .unwrap()
            .contains(&json!(warning))
        {
            info["warnings"]
                .as_array_mut()
                .unwrap()
                .push(json!(warning));
        }
    }

    info["bakedEffects"] = if info["effectsBakedIntoAsset"] == true {
        info["effects"]
            .as_array()
            .map(|es| {
                Value::Array(
                    es.iter()
                        .filter(|e| e["visible"] != false)
                        .cloned()
                        .collect(),
                )
            })
            .unwrap_or(Value::Null)
    } else {
        json!([])
    };
    if let Some(effects) = info["bakedEffects"].as_array().cloned() {
        let warnings = info["warnings"].as_array_mut().unwrap();
        for (kind, warning) in [("BLUR", "blur-baked-in"), ("SHADOW", "shadow-baked-in")] {
            if effects
                .iter()
                .any(|e| e["type"].as_str().is_some_and(|s| s.contains(kind)))
                && !warnings.contains(&json!(warning))
            {
                warnings.push(json!(warning));
            }
        }
    }
    if representation.starts_with("original-fill") {
        let mut specific = Vec::new();
        if info["scaleMode"] == "CROP" {
            specific.push("crop-transform-not-applied");
        }
        if info["extraFills"].as_u64().is_some_and(|n| n > 0) {
            specific.push("extra-fills-not-applied");
        }
        if info["effects"]
            .as_array()
            .is_some_and(|es| es.iter().any(|e| e["visible"] != false))
        {
            specific.push("effects-not-applied");
        }
        for warning in specific {
            if !info["warnings"]
                .as_array()
                .unwrap()
                .contains(&json!(warning))
            {
                info["warnings"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(warning));
            }
        }
    }
    // Exactness is scoped to the returned representation, never canvas parity.
    let exact = measured
        && matches!(
            representation.as_str(),
            "native-node" | "native-visible" | "original-fill"
        )
        && info["dimensionEvidence"] == "encoded-byte-header";
    let transcoded = representation.ends_with("-transcoded") && measured;
    let lossless = info["compression"]["lossless"] == true && info["resize"]["applied"] != true;
    info["fidelity"] = json!(if transcoded {
        if lossless {
            "lossless-pixels"
        } else {
            "approximate"
        }
    } else if exact {
        "exact"
    } else {
        "unverified"
    });
    info["fidelityScope"] = json!(
        if transcoded && representation.starts_with("native-visible") {
            "decoded-selected-ancestor-subtree-PNG-to-WebP; outside-scope-compositing-unverified"
        } else if transcoded && info["compression"]["codec"] == "explicit-source-codec-conversion" {
            "decoded-full-original-source-explicit-codec-conversion; source-byte-identity-not-preserved"
        } else if transcoded && native {
            "decoded-native-node-PNG-to-WebP; source-PNG-byte-identity-not-preserved"
        } else if transcoded {
            "decoded-original-fill-to-WebP; crop-transform-effects-unapplied; source-byte-identity-not-preserved"
        } else if exact && representation == "native-visible" {
            "selected-ancestor-subtree-native-export; outside-scope-compositing-unverified"
        } else if exact && native {
            "native-node-export-at-requested-settings"
        } else if exact {
            "original-source-bytes"
        } else {
            "unknown"
        }
    );
    info["fidelityEvidence"] = if exact {
        info["byteEvidence"].clone()
    } else {
        Value::Null
    };
    info["canvasFidelity"] = json!("unverified");
    info["canvasFidelityReason"] = json!(
        "No independent matching canvas reference was compared; source identity and native node export do not prove ancestor compositing equality."
    );
    info["dimensionValidation"] = if representation.starts_with("original-fill") {
        json!({"status":"not-applicable","scope":"intrinsic-full-source or explicitly resized derivative; compare effective scale against display box"})
    } else {
        match (
            (if representation.starts_with("native-visible") {
                &info["captureBox"]
            } else {
                &info["nodeSize"]
            })["width"]
                .as_f64(),
            (if representation.starts_with("native-visible") {
                &info["captureBox"]
            } else {
                &info["nodeSize"]
            })["height"]
                .as_f64(),
            info["outputSize"]["width"].as_f64(),
            info["outputSize"]["height"].as_f64(),
        ) {
            (Some(w), Some(h), Some(ow), Some(oh)) => {
                let scale = if info["scaleSemantics"] == "intrinsic-original-bytes-no-resize" {
                    1.0
                } else {
                    info["requestedScale"].as_f64().unwrap_or(1.0)
                };
                json!({"expectedLayoutPixels":{"width":w*scale,"height":h*scale},"actualPixels":{"width":ow,"height":oh},"status":if (ow-w*scale).abs() <= 1.0 && (oh-h*scale).abs() <= 1.0 {"match"} else {"mismatch"},"scope":"layout-box; native effects or rotation can extend rendered bounds"})
            }
            _ => json!({"status":"unknown"}),
        }
    };
    if info["dimensionValidation"]["status"] == "mismatch"
        && !info["warnings"]
            .as_array()
            .unwrap()
            .contains(&json!("output-dimensions-differ-from-layout"))
    {
        info["warnings"]
            .as_array_mut()
            .unwrap()
            .push(json!("output-dimensions-differ-from-layout"));
    }
}

/// Naming uses hierarchy and source identity, independently of binary delivery.
pub fn suggested_asset_filename(snapshot: &Snapshot, asset: &AssetManifestEntry) -> Option<String> {
    let node = snapshot.nodes.get(&asset.node_id)?;
    let mut parts = Vec::new();
    let mut current = Some(node.id.clone());
    let mut seen = std::collections::BTreeSet::new();
    while let Some(id) = current {
        if !seen.insert(id.clone()) || seen.len() > 32 {
            break;
        }
        let Some(n) = snapshot.nodes.get(&id) else {
            break;
        };
        if parts.len() < 4 {
            parts.push(filename_part(
                n.typed_view().string("name").unwrap_or("layer"),
            ));
        }
        current = n
            .typed_view()
            .string("parentId")
            .map(str::to_owned)
            .or_else(|| {
                snapshot
                    .nodes
                    .values()
                    .find(|p| p.typed_view().child_ids().any(|child| child == id))
                    .map(|p| p.id.clone())
            });
    }
    if let Some(hierarchy) = asset
        .render_info
        .as_ref()
        .and_then(|i| i["layerHierarchy"].as_array())
    {
        let runtime = hierarchy
            .iter()
            .rev()
            .filter_map(Value::as_str)
            .take(4)
            .map(filename_part)
            .collect::<Vec<_>>();
        if runtime.len() > parts.len() {
            parts = runtime;
        }
    }
    parts.reverse();
    let identity = hash(asset.asset_id.as_bytes());
    Some(format!(
        "{}-{}-{}.{}",
        parts.join("-"),
        &identity[..16],
        if asset.render_mode.is_original()
            || asset
                .render_info
                .as_ref()
                .is_some_and(|i| i["representation"] == "original-fill")
        {
            "fill"
        } else {
            "node"
        },
        asset.format.unwrap_or(AssetFormat::Png).extension()
    ))
}

/// Add snapshot context and safe filename suggestions without changing byte identity
/// or choosing a filesystem destination. Live export observations take precedence.
fn has_child_text(snapshot: &Snapshot, node_id: &str) -> bool {
    let mut pending = vec![node_id];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(node) = snapshot.nodes.get(id) {
            if node.typed_view().bool("visible") == Some(false) {
                continue;
            }
            if id != node_id && node.typed_view().node_type() == "TEXT" {
                return true;
            }
            pending.extend(node.typed_view().child_ids());
        }
    }
    false
}

pub fn annotate_assets(snapshot: &Snapshot, assets: &mut [AssetManifestEntry]) {
    let parents: std::collections::BTreeMap<_, _> = snapshot
        .nodes
        .values()
        .flat_map(|n| {
            n.typed_view()
                .child_ids()
                .map(|id| (id.to_owned(), n.id.clone()))
                .collect::<Vec<_>>()
        })
        .collect();
    for asset in assets {
        let Some(node) = snapshot.nodes.get(&asset.node_id) else {
            continue;
        };
        let view = node.typed_view();
        let mut parts = Vec::new();
        let mut current = Some(node.id.clone());
        let mut seen = std::collections::BTreeSet::new();
        while let Some(id) = current {
            if !seen.insert(id.clone()) || seen.len() > 32 {
                break;
            }
            let Some(n) = snapshot.nodes.get(&id) else {
                break;
            };
            if parts.len() < 4 {
                parts.push(filename_part(
                    n.typed_view().string("name").unwrap_or("layer"),
                ));
            }
            current = n
                .typed_view()
                .string("parentId")
                .map(str::to_owned)
                .or_else(|| parents.get(&id).cloned());
        }
        // A selected leaf's snapshot can omit its ancestors. The export script
        // already traverses them read-only; use its bounded name path when it
        // provides more context than this snapshot, through the same sanitizer.
        if let Some(hierarchy) = asset
            .render_info
            .as_ref()
            .and_then(|info| info["layerHierarchy"].as_array())
        {
            let runtime_parts = hierarchy
                .iter()
                .rev()
                .filter_map(Value::as_str)
                .take(4)
                .map(filename_part)
                .collect::<Vec<_>>();
            if runtime_parts.len() > parts.len() {
                parts = runtime_parts;
            }
        }
        parts.reverse();
        asset.suggested_filename = suggested_asset_filename(snapshot, asset);
        let bytes = asset
            .data_base64
            .as_deref()
            .and_then(|s| STANDARD.decode(s).ok())
            .filter(|b| {
                asset.status == crate::AssetStatus::Exported
                    && asset.byte_length == Some(b.len())
                    && asset.sha256.as_deref() == Some(hash(b).as_str())
            });
        let observed = asset.render_info.take();
        let mut info = measured_info(
            asset.render_mode,
            asset.format.unwrap_or(AssetFormat::Png),
            asset.scale.unwrap_or(2.0),
            observed,
            bytes.as_deref(),
        );
        if info["nodeSize"].is_null() {
            info["nodeSize"] = size(view.number("width"), view.number("height"));
        }
        if info["sourceProperties"].is_null() {
            let paint = asset
                .field
                .strip_prefix("fills/")
                .and_then(|s| s.parse::<usize>().ok())
                .and_then(|i| view.value("fills")?.get(i));
            info["sourceProperties"] = json!({"scaleMode":paint.and_then(|p|p.get("scaleMode")),
                "imageTransform":paint.and_then(|p|p.get("imageTransform")), "effects":view.value("effects"),
                "isMask":view.value("isMask"), "context":"snapshot-only-ancestor-mask-scope-unknown"});
        }
        info["sourceKind"] = json!(asset.source_kind);
        info["sourceVisibility"] = json!(if asset.source_kind == "image-fill"
            && asset_excluded(snapshot, &asset.node_id, &asset.field)
        {
            "hidden-or-transparent-in-document; source-read-independent-of-visibility"
        } else {
            "not-excluded-by-acquired-properties"
        });
        info["recommendedFormat"] = json!(if asset.source_kind == "vector-node" {
            "svg"
        } else {
            "webp"
        });
        info["recommendedRenderMode"] = json!(if asset.source_kind == "image-fill" {
            "original-source"
        } else {
            "node"
        });
        info["instancePath"] = json!(parts);
        info["suggestedFilename"] = json!(asset.suggested_filename);
        info["formatRecommendationEvidence"] = json!(if asset.source_kind == "vector-node" {
            "Vector asset: retain SVG when the native export is usable; inspect effects and embedded rasters."
        } else {
            include_str!("text/asset_render_info-annotate_assets-012d4cd1.md")
        });
        if info["extraFills"].is_null() {
            info["extraFills"] = view
                .value("fills")
                .and_then(Value::as_array)
                .map(|fs| {
                    json!(
                        fs.iter()
                            .filter(|p| p["visible"] != false)
                            .count()
                            .saturating_sub(usize::from(asset.field.starts_with("fills/")))
                    )
                })
                .unwrap_or(Value::Null);
        }
        if asset.status == crate::AssetStatus::Failed {
            info["representation"] = json!("not-exported");
            info["byteEvidence"] = Value::Null;
        }
        compare(&mut info);
        if !asset.render_mode.is_original() && has_child_text(snapshot, &asset.node_id) {
            let warning = json!(
                "node-render-bakes-child-text; use original-source IMAGE fill for backgrounds"
            );
            let warnings = info["warnings"].as_array_mut().unwrap();
            if !warnings.contains(&warning) {
                warnings.push(warning);
            }
        }
        asset.render_info = Some(info);
        if let Some(diagnostics) = &mut asset.export_diagnostics {
            diagnostics.render_info = asset.render_info.clone();
        }
    }
}

/// Enumerate every IMAGE paint in the acquired snapshot, including paints below
/// a flattened manifest boundary. No property heuristic proves canvas equality.
pub fn image_fill_audit(snapshot: &Snapshot, captures: &[AssetManifestEntry]) -> Value {
    let mut entries = Vec::new();
    let mut node_count = 0;
    let mut reason_counts = std::collections::BTreeMap::<String, usize>::new();
    for node in snapshot.nodes.values() {
        if !crate::assets::in_scope(snapshot, &node.id) {
            continue;
        }
        let view = node.typed_view();
        let Some(fills) = view.value("fills").and_then(Value::as_array) else {
            continue;
        };
        let images = fills
            .iter()
            .enumerate()
            .filter(|(_, p)| p["type"] == "IMAGE")
            .collect::<Vec<_>>();
        if images.is_empty() {
            continue;
        }
        node_count += 1;
        for (index, paint) in images {
            let id = format!("{}:fills:{index}", node.id);
            let capture = captures.iter().find(|a| a.asset_id == id);
            let mut reasons = Vec::<&str>::new();
            let mut unknown = vec!["canvas-reference-not-compared"];
            if let Some(transform) = paint.get("imageTransform") {
                let identity = transform.as_array().is_some_and(|rows| {
                    rows.len() == 2
                        && rows.iter().enumerate().all(|(i, r)| {
                            r.as_array().is_some_and(|r| {
                                r.len() == 3
                                    && r.iter().enumerate().all(|(j, v)| {
                                        v.as_f64() == Some([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]][i][j])
                                    })
                            })
                        })
                });
                if !identity {
                    reasons.push("image-transform");
                }
            } else {
                unknown.push("image-transform-unknown");
            }
            match paint["scaleMode"].as_str() {
                Some("FIT" | "CROP" | "TILE") => reasons.push("scale-mode"),
                None => unknown.push("scale-mode-unknown"),
                _ => (),
            }
            if fills.iter().filter(|p| p["visible"] != false).count() > 1 {
                reasons.push("multiple-fills");
            }
            match view.value("effects").and_then(Value::as_array) {
                Some(es) if es.iter().any(|e| e["visible"] != false) => reasons.push("effects"),
                None => unknown.push("effects-unknown"),
                _ => (),
            }
            if view.bool("isMask") == Some(true) {
                reasons.push("mask");
            }
            let info = capture.and_then(|a| a.render_info.as_ref());
            if info.is_none_or(|i| i["outputSize"].is_null()) {
                unknown.push("output-dimensions-unknown");
            }
            if info.is_none_or(|i| !i["sourceProperties"]["ancestors"].is_array()) {
                unknown.push("ancestor-dependencies-unknown");
            }
            if capture.is_some_and(|a| a.status == crate::AssetStatus::Failed) {
                reasons.push("export-failed");
            }
            if let Some(warnings) = info.and_then(|i| i["warnings"].as_array()) {
                for warning in warnings.iter().filter_map(Value::as_str) {
                    *reason_counts.entry(warning.to_owned()).or_default() += 1;
                }
            }
            for reason in reasons.iter().chain(&unknown) {
                *reason_counts.entry((*reason).to_owned()).or_default() += 1;
            }
            entries.push(json!({"assetId":id,"nodeId":node.id,"fillIndex":index,"sourceKind":"image-fill",
                "status":capture.map(|a|a.status),"format":capture.and_then(|a|a.format),"quality":capture.and_then(|a|a.quality),"lossless":capture.and_then(|a|a.lossless),
                "nodeSize":size(view.number("width"), view.number("height")),"nodeSizeEvidence":"acquired-node-layout",
                "sourcePixelSize":info.map(|i|&i["sourcePixelSize"]),"nodeBox":size(view.number("width"),view.number("height")),
                "sourceSize":info.map(|i|&i["sourceSize"]),"sourceSizeEvidence":info.map(|i|i["sourceSizeEvidence"].clone()).filter(|v|!v.is_null()).unwrap_or(json!("unknown; original source not inspected")),
                "outputSize":info.map(|i|&i["outputSize"]),"outputSizeEvidence":info.map(|i|i["dimensionEvidence"].clone()).filter(|v|!v.is_null()).unwrap_or(json!("unknown; no validated capture")),
                "scaleMode":paint.get("scaleMode"),"imageTransform":paint.get("imageTransform"),
                "visibleSourceRatio":info.map(|i|&i["visibleSourceRatio"]),"visibleSourceRatioEvidence":info.map(|i|&i["visibleSourceRatioEvidence"]),
                "instancePath":info.map(|i|&i["instancePath"]),"suggestedFilename":capture.and_then(|a|a.suggested_filename.as_ref()),
                "effectiveScale":info.map(|i|&i["effectiveScale"]),"sourceEffectiveScale":info.map(|i|&i["sourceEffectiveScale"]),"outputEffectiveScale":info.map(|i|&i["outputEffectiveScale"]),
                "cropApplied":info.map(|i|&i["cropApplied"]),"effectsBaked":info.map(|i|&i["effectsBaked"]),"effectsAppliedBy":info.map(|i|&i["effectsAppliedBy"]),
                "effects":view.value("effects"),"recommendedFormat":"webp","recommendedRenderMode":"original-source",
                "recommendationEvidence":"IMAGE paint; raster content type unknown. Use lossless for text/crisp edges, inspect lossy quality; alpha preserved. Retain usable SVG for vectors.",
                "renderInfo":info,
                "fidelity":info.map(|i| &i["fidelity"]),"fidelityScope":info.map(|i| &i["fidelityScope"]),
                "canvasFidelity":"unverified","needsReview":true,"reviewReasons":reasons,"unknownReasons":unknown,
                "instanceOverride":info.map(|i| i["instanceOverride"].clone()).unwrap_or(json!("unknown"))}));
        }
    }
    let total = entries.len();
    let property_risk = entries
        .iter()
        .filter(|e| e["reviewReasons"].as_array().is_some_and(|r| !r.is_empty()))
        .count();
    let property_risk_nodes = entries
        .iter()
        .filter(|e| e["reviewReasons"].as_array().is_some_and(|r| !r.is_empty()))
        .filter_map(|e| e["nodeId"].as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    json!({"scope":"all-IMAGE-paints-in-acquired-snapshot","total":total,"imageFillNodes":node_count,"imageFills":total,
        "exact":0,"exactScope":"independently-compared-canvas-pixels","needsReview":total,"unknown":total,
        "propertyRiskNodes":property_risk_nodes,"propertyRiskFills":property_risk,"representationExact":captures.iter().filter(|a| a.source_kind == "image-fill" && a.render_info.as_ref().is_some_and(|i| i["fidelity"] == "exact")).count(),
        "snapshotNodeCount":snapshot.nodes.len(),"snapshotDiagnosticCount":snapshot.diagnostics.len(),
        "completeness":if snapshot.diagnostics.is_empty(){"acquired-snapshot-only; full-file-coverage-unverified"}else{"acquired-snapshot-with-diagnostics; coverage-may-be-incomplete"},
        "reviewReasonCounts":reason_counts,"assets":entries,
        "summary":format!("{node_count} image-fill nodes / {total} image fills: 0 canvas-exact, {total} need review ({total} canvas-unverified)")})
}
