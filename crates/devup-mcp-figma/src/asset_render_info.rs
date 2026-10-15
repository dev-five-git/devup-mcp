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

pub(crate) fn measured_info(
    mode: AssetRenderMode,
    format: AssetFormat,
    scale: u8,
    observed: Option<Value>,
    bytes: Option<&[u8]>,
) -> Value {
    let mut info = observed
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    info["requestedScale"] = json!(scale);
    let effective = if info["representation"] == "original-fill" {
        AssetRenderMode::Fill
    } else {
        mode
    };
    info["scaleSemantics"] = json!(match (effective, format) {
        (AssetRenderMode::Fill, _) => "intrinsic-original-bytes-no-resize",
        (_, AssetFormat::Png | AssetFormat::Jpg) => "native-node-SCALE-constraint",
        _ => "not-applied-to-vector-document",
    });
    if info.get("representation").is_none() {
        info["representation"] = json!("unknown");
    }
    info["outputSize"] = bytes
        .filter(|_| matches!(format, AssetFormat::Png | AssetFormat::Jpg))
        .and_then(|bytes| {
            image::ImageReader::new(Cursor::new(bytes))
                .with_guessed_format()
                .ok()?
                .into_dimensions()
                .ok()
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
    if info["scaleSemantics"] == "intrinsic-original-bytes-no-resize" {
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
        "native-node" => "node-render",
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
    info["visibleSourceRatio"] = Value::Null;
    info["visibleSourceRatioEvidence"] =
        json!("not-computed; transform-coefficients-are-not-visible-percentages");
    if info["extraFills"].is_null() {
        info["extraFills"] = Value::Null;
    }
    if info["instanceOverride"].is_null() {
        info["instanceOverride"] = json!("unknown");
        info["instanceOverrideEvidence"] = json!("no-component-paint-comparison");
    }
    let measured = info["byteEvidence"].is_object() && representation != "not-exported";
    let native = representation == "native-node";
    info["effectsBakedIntoAsset"] = json!(measured && native && !info["outputSize"].is_null());
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
    if representation == "original-fill" {
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
        && matches!(representation.as_str(), "native-node" | "original-fill")
        && info["dimensionEvidence"] == "encoded-byte-header";
    info["fidelity"] = json!(if exact { "exact" } else { "unverified" });
    info["fidelityScope"] = json!(if exact && native {
        "native-node-export-at-requested-settings"
    } else if exact {
        "original-source-bytes"
    } else {
        "unknown"
    });
    info["fidelityEvidence"] = if exact {
        info["byteEvidence"].clone()
    } else {
        Value::Null
    };
    info["canvasFidelity"] = json!("unverified");
    info["canvasFidelityReason"] = json!(
        "No independent matching canvas reference was compared; source identity and native node export do not prove ancestor compositing equality."
    );
    info["dimensionValidation"] = match (
        info["nodeSize"]["width"].as_f64(),
        info["nodeSize"]["height"].as_f64(),
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

/// Add snapshot context and safe filename suggestions without changing byte identity
/// or choosing a filesystem destination. Live export observations take precedence.
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
        let identity = Sha256::digest(asset.asset_id.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        asset.suggested_filename = Some(format!(
            "{}-{}-{}.{}",
            parts.join("-"),
            &identity[..16],
            if asset.render_mode == AssetRenderMode::Fill
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
        ));
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
            asset.scale.unwrap_or(1),
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
