//! Acquisition evidence passed to code generation and the CSS mapping resource.
use devup_mcp_devup_ui::codegen::{AssetPixelSize, AssetRepresentationPolicy, AssetVariant};
use devup_mcp_figma::{AssetManifestEntry, AssetStatus, Snapshot};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) fn variant_path(path: &str) -> String {
    path.rsplit_once('.').map_or_else(
        || format!("{path}@2x.webp"),
        |(stem, ext)| format!("{stem}@2x.{ext}"),
    )
}

pub(super) fn canonical_path(
    snapshot: &Snapshot,
    asset: &AssetManifestEntry,
    per_node: bool,
) -> Option<String> {
    let path = if asset.render_mode.is_original() {
        asset
            .suggested_filename
            .clone()
            .or_else(|| devup_mcp_figma::suggested_asset_filename(snapshot, asset))
            .map(|name| format!("/images/{name}"))
    } else {
        asset
            .field
            .strip_prefix("fills/")
            .and_then(|s| s.parse::<usize>().ok())
            .and_then(|i| {
                devup_mcp_devup_ui::codegen::image_fill_path(snapshot, &asset.node_id, i, per_node)
            })
            .or_else(|| devup_mcp_devup_ui::codegen::asset_path(snapshot, &asset.node_id, per_node))
            .or_else(|| {
                asset
                    .suggested_filename
                    .clone()
                    .or_else(|| devup_mcp_figma::suggested_asset_filename(snapshot, asset))
                    .map(|name| format!("/images/{name}"))
            })
    }?;
    let path = if let Some(format) = asset.format {
        path.rsplit_once('.').map_or_else(
            || format!("{path}.{}", format.extension()),
            |(stem, _)| format!("{stem}.{}", format.extension()),
        )
    } else {
        path
    };
    Some(super::host_safe_asset_path(&path))
}

pub(super) fn policies(
    snapshot: &Snapshot,
    assets: &[AssetManifestEntry],
    per_node: bool,
) -> BTreeMap<String, AssetRepresentationPolicy> {
    let size = |v: &Value| {
        serde_json::from_value::<AssetPixelSize>(v.clone())
            .ok()
            .filter(|s| {
                s.width.is_finite() && s.height.is_finite() && s.width > 0.0 && s.height > 0.0
            })
    };
    let mut result = BTreeMap::new();
    let mut annotated = assets.to_vec();
    devup_mcp_figma::annotate_assets(snapshot, &mut annotated);
    for asset in annotated
        .iter()
        .filter(|a| a.status == AssetStatus::Exported)
    {
        let info = asset.render_info.as_ref().cloned().unwrap_or(Value::Null);
        let path = asset
            .path
            .clone()
            .or_else(|| canonical_path(snapshot, asset, per_node));
        let mut variants = Vec::new();
        if asset.srcset
            && let (Some(path), Some(output)) = (path.as_ref(), size(&info["outputSize"]))
        {
            variants.push(AssetVariant {
                path: path.clone(),
                width: output.width as u32,
                height: output.height as u32,
                density: 1.0,
            });
            for variant in &asset.variants {
                variants.push(AssetVariant {
                    path: variant.path.clone().unwrap_or_else(|| variant_path(path)),
                    width: variant.width,
                    height: variant.height,
                    density: variant.density,
                });
            }
        }
        let policy = AssetRepresentationPolicy {
            render_mode: asset.render_mode,
            effects_baked: info["effectsBaked"]
                .as_bool()
                .or_else(|| info["effectsBakedIntoAsset"].as_bool()),
            source_pixel_size: size(&info["sourcePixelSize"]),
            output_pixel_size: size(&info["outputSize"]),
            node_box: size(&info["nodeBox"]),
            asset_path: path,
            variants,
            capture_node_id: info["captureNodeId"].as_str().map(str::to_owned),
            capture_box: size(&info["captureBox"]),
            context_scope: info["contextScope"].as_str().map(str::to_owned),
        };
        if asset.field == "node" {
            result.insert(asset.node_id.clone(), policy.clone());
        }
        result.insert(asset.asset_id.clone(), policy);
    }
    result
}

pub(super) fn css_map(snapshot: &Snapshot, assets: &[AssetManifestEntry], per_node: bool) -> Value {
    let policies = policies(snapshot, assets, per_node);
    let entries = assets
        .iter()
        .filter_map(|asset| {
            let node = snapshot.nodes.get(&asset.node_id)?;
            if asset.source_kind != "image-fill" && !asset.render_mode.is_original() {
                return None;
            }
            let policy = policies.get(&asset.asset_id).cloned().unwrap_or_default();
            let path = policy.asset_path.clone().unwrap_or_default();
            let mut mapping = devup_mcp_devup_ui::codegen::asset_css_map(
                &serde_json::to_value(node).ok()?,
                &asset.asset_id,
                &path,
                &policy,
            );
            mapping["status"] = json!(asset.status);
            mapping["usageAspectRatio"] = json!(asset.usage_aspect_ratio);
            mapping["cssApplicationEvidence"] = json!(
                "recommended-recipe; emitted-properties are recorded separately in sourceMap"
            );
            mapping["errorCode"] = json!(asset.error_code);
            mapping["suggestedFilename"] = json!(
                asset
                    .suggested_filename
                    .clone()
                    .or_else(|| devup_mcp_figma::suggested_asset_filename(snapshot, asset))
            );
            mapping["cssSnippet"] = json!(css_snippet(&mapping));
            mapping["instancePath"] = asset
                .render_info
                .as_ref()
                .map(|i| i["instancePath"].clone())
                .unwrap_or(Value::Null);
            Some(mapping)
        })
        .collect::<Vec<_>>();
    json!({"version":1,"policy":"original-source-with-CSS-effects","scope":"acquired-IMAGE-sources; parent/canvas pixel equality unverified","assets":entries})
}

fn css_snippet(mapping: &Value) -> String {
    let mut snippet = String::new();
    for (part, selector) in [
        ("wrapper", ".asset"),
        ("effects", ".asset"),
        ("clip", ".asset-clip"),
        ("image", ".asset-image"),
    ] {
        let Some(properties) = mapping["css"][part].as_object() else {
            continue;
        };
        if properties.is_empty() {
            continue;
        }
        snippet.push_str(selector);
        snippet.push_str(" { ");
        for (name, value) in properties {
            let name = match name.as_str() {
                "objectPos" => "object-position".into(),
                "w" => "width".into(),
                "h" => "height".into(),
                "minW" => "min-width".into(),
                "minH" => "min-height".into(),
                "bg" => "background".into(),
                _ => name
                    .chars()
                    .flat_map(|c| {
                        if c.is_ascii_uppercase() {
                            vec!['-', c.to_ascii_lowercase()]
                        } else {
                            vec![c]
                        }
                    })
                    .collect::<String>(),
            };
            if let Some(value) = value.as_str() {
                snippet.push_str(&format!("{name}: {value}; "));
            }
        }
        snippet.push_str("}\n");
    }
    snippet
}

pub(super) fn enrich_audit(audit: &mut Value, css_map: &Value) {
    for item in audit["assets"].as_array_mut().into_iter().flatten() {
        let Some(mapping) = css_map["assets"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|m| m["assetId"] == item["assetId"])
        else {
            continue;
        };
        item["recommendedCss"] = mapping["css"].clone();
        item["cssSnippet"] = mapping["cssSnippet"].clone();
        item["visibleSourceRatio"] = mapping["visibleSourceRatio"].clone();
        item["cssMapping"] = mapping.clone();
        let source = item["sourcePixelSize"]["aspectRatio"].as_f64();
        let node = item["nodeBox"]["aspectRatio"].as_f64();
        // A recommendation recipe is distinct from a caller-declared use site.
        let recipe = mapping["nodeBox"]["width"]
            .as_f64()
            .zip(mapping["nodeBox"]["height"].as_f64())
            .map(|(w, h)| w / h)
            .filter(|r| r.is_finite() && *r > 0.0);
        let usage = mapping["usageAspectRatio"].as_f64();
        item["aspectRatioComparison"] = json!({"source":source,"node":node,"usage":usage,"usageEvidence":if usage.is_some(){"caller-declared-use-site; browser-unverified"}else{"unknown; no use-site measurement or declaration supplied"},"recipeAspectRatio":recipe,"recipeEvidence":"recommended-CSS-layout-node-box",
            "sourceVsNode":source.zip(node).map(|(s,n)|if(s/n-1.0).abs()>0.01{"different; object-fit or affine crop required"}else{"match"}),
            "nodeVsUsage":node.zip(usage).map(|(n,u)|if(n/u-1.0).abs()>0.01{"mismatch"}else{"match"}),"canvasEquality":"unverified"});
    }
}

/// Preserve native alternatives except wrapper boundaries replaced by acquired
/// original sources; preset inventories are exclusively the full IMAGE census.
pub(super) fn discovery(
    snapshot: &Snapshot,
    captures: &[AssetManifestEntry],
    source_only: bool,
) -> devup_mcp_figma::AssetManifest {
    let census = devup_mcp_figma::discover_image_fill_assets(snapshot);
    let mut manifest = devup_mcp_figma::discover_asset_manifest(snapshot);
    if source_only {
        manifest.assets = census;
        return manifest;
    }
    manifest.assets.retain(|asset| {
        if asset.field != "node" || captures.iter().any(|c| c.asset_id == asset.asset_id) {
            return true;
        }
        let mut pending = snapshot
            .nodes
            .get(&asset.node_id)
            .map(|n| {
                n.typed_view()
                    .child_ids()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut seen = std::collections::BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            if captures.iter().any(|a| {
                a.node_id == id && a.render_mode == devup_mcp_figma::AssetRenderMode::OriginalSource
            }) {
                return false;
            }
            if let Some(node) = snapshot.nodes.get(&id) {
                pending.extend(node.typed_view().child_ids().map(str::to_owned));
            }
        }
        true
    });
    for source in census {
        if !manifest
            .assets
            .iter()
            .any(|a| a.asset_id == source.asset_id)
        {
            manifest.assets.push(source);
        }
    }
    manifest
}
