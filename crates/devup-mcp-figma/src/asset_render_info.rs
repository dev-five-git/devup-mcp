//! Export observations are evidence, never a claim of canvas fidelity.
use std::io::Cursor;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::{AssetFormat, AssetManifestEntry, AssetRenderMode, Snapshot};

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
    info["scaleSemantics"] = json!(match (mode, format) {
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
            Some("aspect-ratio-mismatch" | "aspect-ratio-unknown")
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
    info["fidelity"] = json!("unverified");
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
            if asset.render_mode == AssetRenderMode::Fill {
                "fill"
            } else {
                "node"
            },
            asset.format.unwrap_or(AssetFormat::Png).extension()
        ));
        let bytes = asset
            .data_base64
            .as_deref()
            .and_then(|s| STANDARD.decode(s).ok());
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
        compare(&mut info);
        asset.render_info = Some(info);
        if let Some(diagnostics) = &mut asset.export_diagnostics {
            diagnostics.render_info = asset.render_info.clone();
        }
    }
}
