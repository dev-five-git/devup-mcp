//! Image bytes and the layout painted with those bytes are separate contracts.
//! Figma's Paint API defines imageTransform for CROP, rotation for FILL/FIT,
//! and paint order/blending: https://developers.figma.com/docs/plugins/api/Paint/
use std::collections::{BTreeMap, BTreeSet};

use devup_mcp_figma::{AssetRenderMode, Diagnostic, DiagnosticSeverity, RawNode, Snapshot};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{component::Prop, layout::string_prop, style};

// Matrix coefficients need more precision than ordinary CSS lengths.
fn number(value: f64) -> String {
    let text = format!("{value:.10}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if matches!(text, "-0" | "") {
        "0".into()
    } else {
        text.into()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetPixelSize {
    #[serde(alias = "w")]
    pub width: f64,
    #[serde(alias = "h")]
    pub height: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AssetVariant {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub density: f64,
}

/// Supplied by acquisition, keyed by the instance-specific manifest assetId.
/// Missing evidence is retained as unknown; a filename never proves baking.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AssetRepresentationPolicy {
    pub render_mode: AssetRenderMode,
    #[serde(alias = "effectsBakedIntoAsset")]
    pub effects_baked: Option<bool>,
    #[serde(alias = "sourceSize")]
    pub source_pixel_size: Option<AssetPixelSize>,
    #[serde(alias = "pixelSize", alias = "outputSize")]
    pub output_pixel_size: Option<AssetPixelSize>,
    #[serde(alias = "nodeSize")]
    pub node_box: Option<AssetPixelSize>,
    #[serde(alias = "publicPath")]
    pub asset_path: Option<String>,
    pub variants: Vec<AssetVariant>,
    pub capture_node_id: Option<String>,
    pub capture_box: Option<AssetPixelSize>,
    pub context_scope: Option<String>,
}

impl AssetRepresentationPolicy {
    pub(super) fn original(&self) -> bool {
        matches!(
            self.render_mode,
            AssetRenderMode::OriginalSource | AssetRenderMode::Fill
        )
    }
    pub(super) fn incompatible_capture(&self, node_id: &str) -> bool {
        self.render_mode == AssetRenderMode::Visible
            && self
                .capture_node_id
                .as_deref()
                .is_some_and(|id| id != node_id)
    }
}

pub(super) fn policy_for(
    node: &RawNode,
    asset_id: &str,
    policies: &BTreeMap<String, AssetRepresentationPolicy>,
) -> AssetRepresentationPolicy {
    policies
        .get(asset_id)
        .or_else(|| policies.get(&node.id))
        .cloned()
        .unwrap_or_default()
}

pub(super) fn original_image(
    node: &RawNode,
    policies: &BTreeMap<String, AssetRepresentationPolicy>,
) -> bool {
    if policies.get(&node.id).is_some_and(|p| !p.original()) {
        return false;
    }
    node.typed_view()
        .value("fills")
        .and_then(Value::as_array)
        .is_some_and(|fills| {
            let images = fills
                .iter()
                .enumerate()
                .filter(|(_, p)| p["type"] == "IMAGE" && p["visible"] != false)
                .collect::<Vec<_>>();
            !images.is_empty()
                && images.iter().all(|(i, _)| {
                    policy_for(node, &format!("{}:fills:{i}", node.id), policies).original()
                })
        })
}

pub(super) fn selected_image_index(
    node: &RawNode,
    policies: &BTreeMap<String, AssetRepresentationPolicy>,
) -> Option<usize> {
    let fills = node.typed_view().value("fills")?.as_array()?;
    fills
        .iter()
        .enumerate()
        .find(|(i, p)| {
            p["type"] == "IMAGE"
                && p["visible"] != false
                && !policy_for(node, &format!("{}:fills:{i}", node.id), policies).original()
        })
        .map(|(i, _)| i)
        .or_else(|| {
            fills
                .iter()
                .position(|p| p["type"] == "IMAGE" && p["visible"] != false)
        })
}

pub(super) fn asset_kind(
    snapshot: &Snapshot,
    node: &RawNode,
    policies: &BTreeMap<String, AssetRepresentationPolicy>,
) -> Option<style::AssetKind> {
    if policies.get(&node.id).is_some_and(|p| !p.original()) {
        return Some(style::AssetKind::Png);
    }
    let native = node
        .typed_view()
        .value("fills")
        .and_then(Value::as_array)
        .is_some_and(|fs| {
            fs.iter().enumerate().any(|(i, p)| {
                p["type"] == "IMAGE"
                    && p["visible"] != false
                    && !policy_for(node, &format!("{}:fills:{i}", node.id), policies).original()
            })
        });
    if native {
        Some(style::AssetKind::Png)
    } else {
        let kind = style::asset_kind(snapshot, node);
        if kind == Some(style::AssetKind::Png) && !original_image(node, policies) {
            let mut pending = node
                .typed_view()
                .child_ids()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let mut seen = BTreeSet::new();
            while let Some(id) = pending.pop() {
                if !seen.insert(id.clone()) {
                    continue;
                }
                if let Some(child) = snapshot.nodes.get(&id) {
                    if original_image(child, policies) {
                        return None;
                    }
                    pending.extend(child.typed_view().child_ids().map(str::to_owned));
                }
            }
        }
        kind
    }
}

fn fields(node: &Value) -> &Value {
    node.get("fields").unwrap_or(node)
}
fn size(value: &Value) -> Option<AssetPixelSize> {
    let width = value.get("width").or_else(|| value.get("w"))?.as_f64()?;
    let height = value.get("height").or_else(|| value.get("h"))?.as_f64()?;
    (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0)
        .then_some(AssetPixelSize { width, height })
}
fn valid_size(value: Option<AssetPixelSize>) -> Option<AssetPixelSize> {
    value.filter(|s| s.width.is_finite() && s.height.is_finite() && s.width > 0.0 && s.height > 0.0)
}
fn node_size(node: &Value, policy: &AssetRepresentationPolicy) -> Option<AssetPixelSize> {
    valid_size(policy.node_box)
        .or_else(|| size(fields(node)))
        .or_else(|| size(&fields(node)["absoluteBoundingBox"]))
}
fn paint<'a>(node: &'a Value, asset_id: &str) -> Option<&'a Value> {
    let index = asset_id
        .rsplit_once(":fills:")
        .and_then(|(_, s)| s.parse::<usize>().ok());
    let fills = fields(node)["fills"].as_array()?;
    index
        .and_then(|i| fills.get(i))
        .or_else(|| fills.iter().find(|p| p["type"] == "IMAGE"))
}

type Matrix = [[f64; 3]; 2];
const IDENTITY: Matrix = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
fn matrix(value: &Value) -> Option<Matrix> {
    let rows = value.as_array()?;
    if rows.len() != 2 {
        return None;
    }
    let row = |i: usize| -> Option<[f64; 3]> {
        let r = rows[i].as_array()?;
        if r.len() != 3 {
            return None;
        }
        let v = [r[0].as_f64()?, r[1].as_f64()?, r[2].as_f64()?];
        v.iter().all(|v| v.is_finite()).then_some(v)
    };
    Some([row(0)?, row(1)?])
}
fn inverse(m: Matrix) -> Option<Matrix> {
    let [[a, b, c], [d, e, f]] = m;
    let det = a * e - b * d;
    if !det.is_finite() || det.abs() < 1e-10 {
        return None;
    }
    Some([
        [e / det, -b / det, (b * f - c * e) / det],
        [-d / det, a / det, (c * d - a * f) / det],
    ])
}
fn point(m: Matrix, p: [f64; 2]) -> [f64; 2] {
    [
        m[0][0] * p[0] + m[0][1] * p[1] + m[0][2],
        m[1][0] * p[0] + m[1][1] * p[1] + m[1][2],
    ]
}
fn corners(m: Matrix) -> Vec<[f64; 2]> {
    [[0., 0.], [1., 0.], [1., 1.], [0., 1.]]
        .map(|p| point(m, p))
        .to_vec()
}
fn clipped_polygon(mut poly: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    for (axis, edge, sign) in [(0, 0., 1.), (0, 1., -1.), (1, 0., 1.), (1, 1., -1.)] {
        let input = std::mem::take(&mut poly);
        if input.is_empty() {
            break;
        }
        let mut previous = *input.last().unwrap();
        for current in input {
            let inside = |p: [f64; 2]| (p[axis] - edge) * sign >= 0.;
            if inside(current) != inside(previous) {
                let t = (edge - previous[axis]) / (current[axis] - previous[axis]);
                poly.push([
                    previous[0] + t * (current[0] - previous[0]),
                    previous[1] + t * (current[1] - previous[1]),
                ]);
            }
            if inside(current) {
                poly.push(current);
            }
            previous = current;
        }
    }
    poly
}
fn coverage(m: Matrix) -> Value {
    // Map the node's unit-square viewport INTO normalized source coordinates,
    // then intersect with the source unit square. Bounds are not polygon area.
    let polygon = clipped_polygon(corners(m));
    let mut area = 0.;
    for i in 0..polygon.len() {
        let a = polygon[i];
        let b = polygon[(i + 1) % polygon.len()];
        area += a[0] * b[1] - b[0] * a[1];
    }
    let bounds = if polygon.is_empty() {
        json!({"x":0,"y":0,"width":0,"height":0})
    } else {
        let min = |axis: usize| {
            polygon
                .iter()
                .map(|p| p[axis])
                .fold(f64::INFINITY, f64::min)
        };
        let max = |axis: usize| {
            polygon
                .iter()
                .map(|p| p[axis])
                .fold(f64::NEG_INFINITY, f64::max)
        };
        json!({"x":min(0),"y":min(1),"width":max(0)-min(0),"height":max(1)-min(1)})
    };
    json!({"known":true,"w":bounds["width"],"h":bounds["height"],"area":(area.abs()/2.).clamp(0.,1.),"bounds":bounds,"polygon":polygon,"coordinateSpace":"normalized-source-unit-square","method":"viewport-polygon-intersection"})
}

#[derive(Default)]
struct Projection {
    image: BTreeMap<String, String>,
    background: Option<String>,
    forward: Option<Matrix>,
    sampling: Option<Matrix>,
    visible: Value,
    reasons: Vec<String>,
}
fn projection(node: &Value, asset_id: &str, policy: &AssetRepresentationPolicy) -> Projection {
    let mut out = Projection::default();
    out.visible = json!({"known":false,"reason":"source-dimensions-unavailable"});
    if !policy.original() {
        out.image.insert("objectFit".into(), "fill".into());
        out.background = Some("0 0/100% 100% no-repeat".into());
        out.visible = json!({"known":false,"reason":"baked-render-source-coverage-unverified"});
        return out;
    }
    let Some(p) = paint(node, asset_id) else {
        out.reasons.push("image-paint-unavailable".into());
        return out;
    };
    let mode = p["scaleMode"].as_str().unwrap_or("FILL");
    if mode == "CROP" {
        let Some(t) = matrix(&p["imageTransform"]) else {
            out.reasons.push("crop-transform-missing-or-invalid".into());
            out.visible = json!({"known":false,"reason":"crop-transform-missing-or-invalid"});
            return out;
        };
        let Some(forward) = inverse(t) else {
            out.reasons.push("singular-image-transform".into());
            out.visible = json!({"known":false,"reason":"singular-image-transform"});
            return out;
        };
        out.visible = coverage(t);
        out.sampling = Some(t);
        out.forward = Some(forward);
        let [[a, b, c], [d, e, f]] = t;
        if a > 0.
            && e > 0.
            && b.abs() < 1e-10
            && d.abs() < 1e-10
            && ((1. - a).abs() > 1e-10 || c.abs() < 1e-10)
            && ((1. - e).abs() > 1e-10 || f.abs() < 1e-10)
        {
            let x = if (1. - a).abs() < 1e-10 {
                0.
            } else {
                c / (1. - a) * 100.
            };
            let y = if (1. - e).abs() < 1e-10 {
                0.
            } else {
                f / (1. - e) * 100.
            };
            out.background = Some(format!(
                "{}% {}%/{}% {}% no-repeat",
                number(x),
                number(y),
                number(100. / a),
                number(100. / e)
            ));
        }
        out.image.insert("objectFit".into(), "fill".into());
    } else if matches!(mode, "FILL" | "FIT") {
        let fit = if mode == "FIT" { "contain" } else { "cover" };
        out.image.insert("objectFit".into(), fit.into());
        out.image.insert("objectPos".into(), "center".into());
        out.background = Some(format!("center/{fit} no-repeat"));
        // Figma documents imageTransform as CROP-only. An incidental matrix
        // on FILL/FIT is not evidence that its diagonal measures coverage.
        if matrix(&p["imageTransform"]).is_some_and(|t| t != IDENTITY) {
            out.reasons
                .push("image-transform-inapplicable-to-scale-mode".into());
        }
        let rotation = p["rotation"].as_f64().unwrap_or(0.);
        if !rotation.is_finite() || (rotation / 90. - (rotation / 90.).round()).abs() > 1e-8 {
            out.reasons.push("unsupported-paint-rotation".into());
            out.visible = json!({"known":false,"reason":"unsupported-paint-rotation"});
            return out;
        }
        if let (Some(source), Some(box_size)) = (
            valid_size(policy.source_pixel_size),
            node_size(node, policy),
        ) {
            let r = rotation.to_radians();
            let (c, s) = (r.cos(), r.sin());
            let rw = c.abs() * source.width + s.abs() * source.height;
            let rh = s.abs() * source.width + c.abs() * source.height;
            let scale = if mode == "FIT" {
                (box_size.width / rw).min(box_size.height / rh)
            } else {
                (box_size.width / rw).max(box_size.height / rh)
            };
            let a = scale * c * source.width / box_size.width;
            let b = -scale * s * source.height / box_size.width;
            let d = scale * s * source.width / box_size.height;
            let e = scale * c * source.height / box_size.height;
            let forward = [[a, b, (1. - a - b) / 2.], [d, e, (1. - d - e) / 2.]];
            out.sampling = inverse(forward);
            out.visible = coverage(inverse(forward).unwrap());
            if rotation.rem_euclid(360.).abs() > 1e-8 {
                out.forward = Some(forward);
                out.background = None;
                out.image.insert("objectFit".into(), "fill".into());
            }
        } else if rotation != 0. {
            out.reasons
                .push("rotation-requires-source-and-node-dimensions".into());
        }
    } else if mode == "TILE" {
        out.background = Some(if let Some(source) = valid_size(policy.source_pixel_size) {
            let factor = p["scalingFactor"]
                .as_f64()
                .filter(|n| n.is_finite() && *n > 0.)
                .unwrap_or(1.);
            format!(
                "0 0/{}px {}px repeat",
                number(source.width * factor),
                number(source.height * factor)
            )
        } else {
            "repeat".into()
        });
        out.visible = json!({"known":false,"reason":"tile-repetition-has-no-single-crop-polygon"});
        if p["rotation"]
            .as_f64()
            .is_some_and(|r| r.rem_euclid(360.) != 0.)
        {
            out.reasons.push("tile-rotation-unrepresentable".into());
        }
    } else {
        out.visible = json!({"known":false,"reason":"unsupported-image-scale-mode"});
        out.reasons.push("unsupported-image-scale-mode".into());
    }
    if let Some(m) = out.forward {
        if let Some(b) = node_size(node, policy) {
            let [[a, x, tx], [y, d, ty]] = m;
            out.image.insert(
                "transform".into(),
                format!(
                    "translate({}%, {}%) matrix({}, {}, {}, {}, 0, 0)",
                    number(tx * 100.),
                    number(ty * 100.),
                    number(a),
                    number(y * b.height / b.width),
                    number(x * b.width / b.height),
                    number(d)
                ),
            );
            out.image.insert("transformOrigin".into(), "0 0".into());
        } else {
            out.reasons
                .push("affine-projection-requires-node-box".into());
        }
    }
    out
}

/// Reusable acquisition-to-CSS mapping. `node` accepts a RawNode JSON object
/// or its fields. `css` uses DevupUI prop names; intrinsic attributes are separate.
pub fn asset_css_map(
    node: &Value,
    asset_id: &str,
    asset_path: &str,
    policy: &AssetRepresentationPolicy,
) -> Value {
    let f = fields(node);
    let node_id = node["id"]
        .as_str()
        .or_else(|| asset_id.rsplit_once(":fills:").map(|(id, _)| id))
        .unwrap_or(asset_id);
    let p = paint(node, asset_id);
    let mut projected = projection(node, asset_id, policy);
    if policy.original()
        && policy.effects_baked != Some(true)
        && let Some(opacity) = p.and_then(|p| p["opacity"].as_f64()).filter(|n| *n < 1.)
    {
        projected.image.insert("opacity".into(), number(opacity));
    }
    let box_size = node_size(node, policy);
    let path = policy.asset_path.as_deref().unwrap_or(asset_path);
    let baked = policy.effects_baked == Some(true);
    if policy.effects_baked.is_none() {
        projected
            .reasons
            .push("effects-baked-state-unverified".into());
    }
    if policy.incompatible_capture(node_id) {
        projected
            .reasons
            .push("incompatible-visible-capture".into());
    }
    let mut wrapper: BTreeMap<String, String> = BTreeMap::new();
    if let Some(b) = box_size {
        wrapper.insert(
            "aspectRatio".into(),
            format!("{} / {}", number(b.width), number(b.height)),
        );
    }
    wrapper.insert("minW".into(), "0".into());
    wrapper.insert("minH".into(), "0".into());
    let clip=projected.image.contains_key("transform").then(||json!({"pos":"relative","w":"100%","h":"100%","overflow":"hidden","borderRadius":"inherit"}));
    let raw: RawNode = serde_json::from_value(
        json!({"id":node_id,"type":node["type"].as_str().unwrap_or("RECTANGLE"),"fields":f}),
    )
    .expect("synthetic node has required fields");
    let effects = if baked || (!policy.original() && policy.effects_baked.is_none()) {
        BTreeMap::new()
    } else {
        style::image_effect_props(&raw)
    };
    if f["isMask"] == true {
        projected.visible = json!({"known":false,"reason":"mask-aperture-unavailable"});
        projected.reasons.push("mask-aperture-unavailable".into());
    }
    if p.and_then(|p| p["filters"].as_object())
        .is_some_and(|fs| fs.values().any(|v| v.as_f64().is_some_and(|n| n != 0.)))
    {
        projected
            .reasons
            .push("figma-image-filters-unrepresentable".into());
    }
    if matches!(
        node["type"].as_str(),
        Some("VECTOR" | "STAR" | "POLYGON" | "BOOLEAN_OPERATION")
    ) {
        projected
            .reasons
            .push("original-source-vector-shape-unrepresented".into());
    }
    if f["cornerSmoothing"].as_f64().is_some_and(|n| n > 0.) {
        projected
            .reasons
            .push("figma-corner-smoothing-approximated".into());
    }
    if p.and_then(|p| p["blendMode"].as_str())
        .is_some_and(|m| matches!(m, "LINEAR_BURN" | "LINEAR_DODGE"))
    {
        projected
            .reasons
            .push("paint-blend-mode-unrepresentable".into());
    }
    let intrinsic = if policy.original() {
        valid_size(policy.output_pixel_size).or_else(|| valid_size(policy.source_pixel_size))
    } else {
        valid_size(policy.output_pixel_size)
    };
    let base = policy
        .variants
        .iter()
        .find(|v| (v.density - 1.).abs() < 1e-9 && v.width > 0 && v.height > 0)
        .map(|v| AssetPixelSize {
            width: v.width as f64,
            height: v.height as f64,
        })
        .or_else(|| valid_size(policy.output_pixel_size));
    let mut variants = policy
        .variants
        .iter()
        .filter(|v| {
            !v.path.is_empty()
                && v.width > 0
                && v.height > 0
                && v.density.is_finite()
                && v.density > 0.
        })
        .filter(|v| {
            base.is_some_and(|b| {
                (v.width as f64 - b.width * v.density).abs() <= 1.
                    && (v.height as f64 - b.height * v.density).abs() <= 1.
            })
        })
        .collect::<Vec<_>>();
    if variants.len() != policy.variants.len() {
        projected
            .reasons
            .push("variant-density-inconsistent-with-encoded-dimensions".into());
    }
    variants.sort_by(|a, b| a.density.total_cmp(&b.density));
    variants.dedup_by(|a, b| a.density == b.density);
    let src_set = (!variants.is_empty()).then(|| {
        variants
            .iter()
            .map(|v| format!("{} {}x", v.path, number(v.density)))
            .collect::<Vec<_>>()
            .join(", ")
    });
    let sizes = src_set
        .as_ref()
        .and_then(|_| box_size.map(|b| format!("{}px", number(b.width))));
    let effective = |s: Option<AssetPixelSize>| {
        s.zip(box_size).map(|(s,b)|json!({"x":s.width/b.width,"y":s.height/b.height,"minimum":(s.width/b.width).min(s.height/b.height),"basis":"pixel-dimensions-divided-by-layout-node-box"}))
    };
    let source_effective = effective(valid_size(policy.source_pixel_size));
    let output_effective = effective(valid_size(policy.output_pixel_size));
    let projected_scale = |pixels: Option<AssetPixelSize>| {
        pixels.zip(box_size).zip(projected.sampling).map(|((s,b),m)| {
        let a=s.width*m[0][0]/b.width;let c=s.width*m[0][1]/b.height;
        let d=s.height*m[1][0]/b.width;let e=s.height*m[1][1]/b.height;
        let trace=a*a+c*c+d*d+e*e;let det=a*e-c*d;
        json!({"x":a.hypot(d),"y":c.hypot(e),"minimum":((trace-(trace*trace-4.*det*det).max(0.).sqrt())/2.).max(0.).sqrt(),"basis":"singular-values-of-source-pixels-per-node-pixel-normalized-sampling-transform"})
    })
    };
    let projected_source_effective = projected_scale(valid_size(policy.source_pixel_size));
    let projected_output_effective = if policy.original() {
        projected_scale(valid_size(policy.output_pixel_size))
    } else {
        output_effective.clone()
    };
    if let Some(e) = if policy.original() {
        projected_output_effective
            .as_ref()
            .or(projected_source_effective.as_ref())
            .or(source_effective.as_ref())
    } else {
        output_effective.as_ref()
    } {
        if e["minimum"].as_f64().is_some_and(|n| n < 1.5) {
            projected.reasons.push("low-resolution-bitmap".into());
        }
    }
    if intrinsic.is_none() {
        projected
            .reasons
            .push("intrinsic-pixel-dimensions-unavailable".into());
    }
    let property_evidence=effects.iter().map(|(property,value)|json!({"property":property,"value":value,"appliedBy":"css","source":"node.effects/radii/strokes/opacity/blendMode"})).collect::<Vec<_>>();
    json!({"assetId":asset_id,"nodeId":node_id,"instanceIdentity":node_id,"instancePath":f.get("instancePath").or_else(||f.get("parentId")),"assetPath":path,
        "renderMode":policy.render_mode,"effectsBaked":policy.effects_baked,"effectsAppliedBy":if baked{json!("asset")}else if policy.original()||policy.effects_baked==Some(false){json!("css")}else{Value::Null},
        "sourcePixelSize":policy.source_pixel_size,"outputPixelSize":policy.output_pixel_size,"nodeBox":box_size,
        "layoutSizing":{"horizontal":f["layoutSizingHorizontal"],"vertical":f["layoutSizingVertical"]},
        "captureNodeId":policy.capture_node_id,"captureBox":policy.capture_box,"contextScope":policy.context_scope,
        "scaleMode":p.and_then(|p|p.get("scaleMode")),"imageTransform":p.and_then(|p|p.get("imageTransform")),
        "paintLayer":{"fillIndex":asset_id.rsplit_once(":fills:").and_then(|(_,i)|i.parse::<usize>().ok()),"opacity":p.and_then(|p|p.get("opacity")),"blendMode":p.and_then(|p|p.get("blendMode")),"nodeOpacity":f["opacity"],"nodeBlendMode":f["blendMode"]},
        "coordinateSemantics":{"imageTransform":"CROP: normalized node viewport to normalized source; inverse projects source into viewport","cropConventionEvidence":"inferred normalized sampling convention; official Paint API documents applicability but not direction; no canvas parity claim","fillFit":"centered aspect-preserving cover/contain; imageTransform is inapplicable","cssMatrix":"conjugate normalized inverse by node pixel dimensions; percentage translation precedes linear matrix","coverage":"source unit square intersected with transformed viewport polygon","scope":"node paint only; ancestor masks/clips and canvas composition are not established"},
        "visibleSourceRatio":projected.visible,"cropApplied":if policy.original(){json!(false)}else{Value::Null},"cssCropApplied":policy.original()&&projected.forward.is_some(),
        "css":{"wrapper":wrapper,"clip":clip,"image":projected.image,"effects":effects,"background":projected.background},
        "composition":{"applyEffectsTo":"outer-node-after-paint-clipping","paintOrder":"fills array bottom-to-top; live children above paint elements","affineImage":"full source in a 100% sized absolute Image inside a relative overflow-hidden clip","hugAxes":"clip uses measured nodeBox content dimension where the outer HUG axis has no definite CSS size","coverageScope":"rectangular-node-viewport-before-radius-mask-and-ancestor-composition","shapeClippingIncludedInCoverage":false},
        "intrinsicAttributes":intrinsic.map(|s|json!({"width":s.width,"height":s.height})),"srcSet":src_set,"sizes":sizes,
        "sourceEffectiveScale":source_effective,"outputEffectiveScale":output_effective,"projectedSourceEffectiveScale":projected_source_effective,"projectedOutputEffectiveScale":projected_output_effective,"variants":variants,
        "propertyEvidence":property_evidence,"warnings":projected.reasons,"unsupported":projected.reasons.iter().filter(|s|!matches!(s.as_str(),"image-transform-inapplicable-to-scale-mode"|"low-resolution-bitmap"|"intrinsic-pixel-dimensions-unavailable"|"effects-baked-state-unverified")).collect::<Vec<_>>()})
}

pub(super) fn apply_image_props(props: &mut Vec<Prop>, mapping: &Value) {
    for (name, value) in mapping["css"]["image"].as_object().into_iter().flatten() {
        if matches!(name.as_str(), "transform" | "transformOrigin" | "opacity") {
            continue;
        }
        if let Some(v) = value.as_str() {
            string_prop(props, name, v);
        }
    }
    for name in ["width", "height"] {
        if let Some(v) = mapping["intrinsicAttributes"][name].as_f64() {
            string_prop(props, name, number(v));
        }
    }
    for name in ["srcSet", "sizes"] {
        if let Some(v) = mapping[name].as_str() {
            string_prop(props, name, v);
        }
    }
    for name in ["minW", "minH", "aspectRatio"] {
        if let Some(v) = mapping["css"]["wrapper"][name].as_str() {
            string_prop(props, name, v);
        }
    }
}

/// A HUG image contributes its measured display content, rather than the
/// unrelated pixel dimensions of the full source file. Embedded roots keep
/// their host sizing contract and never use this standalone content anchor.
pub(super) fn reserve_hug_content(props: &mut Vec<Prop>, mapping: &Value) {
    if mapping["layoutSizing"]["horizontal"] != "HUG"
        || mapping["layoutSizing"]["vertical"] != "HUG"
        || !mapping["css"]["image"]["transform"].is_null()
        || props
            .iter()
            .any(|(n, _)| matches!(n.as_str(), "w" | "h" | "boxSize"))
    {
        return;
    }
    for (dimension, css) in [("width", "w"), ("height", "h")] {
        if let Some(value) = mapping["nodeBox"][dimension].as_f64() {
            string_prop(props, css, format!("{}px", number(value)));
        }
    }
}

fn element(component: &str, props: &[Prop], children: Option<&str>, depth: usize) -> String {
    let indent = "  ".repeat(depth);
    let (opening, multiline) = super::component::render_props(props, depth);
    match children {
        Some(content) => format!(
            "{indent}<{component}{opening}{}>\n{content}\n{indent}</{component}>",
            if multiline {
                format!("\n{indent}")
            } else {
                String::new()
            }
        ),
        None => format!(
            "{indent}<{component}{opening}{} />",
            if multiline {
                format!("\n{indent}")
            } else {
                String::new()
            }
        ),
    }
}

pub(super) fn render_affine_image(props: &[Prop], mapping: &Value, depth: usize) -> Option<String> {
    mapping["css"]["image"]["transform"].as_str()?;
    let image_names = [
        "src",
        "srcSet",
        "sizes",
        "width",
        "height",
        "objectFit",
        "objectPos",
        "alt",
    ];
    let mut outer = props
        .iter()
        .filter(|(n, _)| !image_names.contains(&n.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    // Layout transforms stay on the outer node; the paint transform belongs
    // only to the inner full-source image.
    if !outer.iter().any(|(n, _)| n == "pos") {
        string_prop(&mut outer, "pos", "relative");
    }
    let mut image = props
        .iter()
        .filter(|(n, _)| image_names.contains(&n.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    for (name, value) in mapping["css"]["image"].as_object().into_iter().flatten() {
        if let Some(v) = value.as_str() {
            string_prop(&mut image, name, v);
        }
    }
    for (name, value) in [
        ("w", "100%"),
        ("h", "100%"),
        ("pos", "absolute"),
        ("left", "0"),
        ("top", "0"),
        ("maxW", "none"),
        ("display", "block"),
    ] {
        string_prop(&mut image, name, value);
    }
    let mut clip = Vec::new();
    for (name, value) in [
        ("w", "100%"),
        ("h", "100%"),
        ("pos", "relative"),
        ("overflow", "hidden"),
        ("borderRadius", "inherit"),
    ] {
        string_prop(&mut clip, name, value);
    }
    for (dimension, css) in [("width", "w"), ("height", "h")] {
        if !outer.iter().any(|(n, _)| n == css || n == "boxSize")
            && let Some(value) = mapping["nodeBox"][dimension].as_f64()
        {
            string_prop(&mut clip, css, format!("{}px", number(value)));
        }
    }
    Some(element(
        "Box",
        &outer,
        Some(&element(
            "Box",
            &clip,
            Some(&element("Image", &image, None, depth + 2)),
            depth + 1,
        )),
        depth,
    ))
}

/// Separate paint elements support per-paint opacity/blending and affine
/// images while keeping live children outside the image's crop wrapper.
pub(super) fn render_paint_layers(
    snapshot: &Snapshot,
    node: &RawNode,
    options: style::StyleOptions<'_>,
    used: &mut BTreeSet<String>,
    depth: usize,
) -> Option<Vec<String>> {
    if style::non_rendering_asset_reason(snapshot, node).is_some() {
        return None;
    }
    let paints = node.typed_view().value("fills")?.as_array()?;
    let images = paints
        .iter()
        .enumerate()
        .filter(|(_, p)| p["type"] == "IMAGE" && p["visible"] != false)
        .collect::<Vec<_>>();
    if images.is_empty()
        || images.iter().any(|(i, _)| {
            !policy_for(
                node,
                &format!("{}:fills:{i}", node.id),
                options.asset_policies,
            )
            .original()
        })
    {
        return None;
    }
    let mut layers = Vec::new();
    for (index, paint) in paints
        .iter()
        .enumerate()
        .filter(|(_, p)| p["visible"] != false && p["opacity"] != 0)
    {
        let mut layer = Vec::new();
        for (name, value) in [
            ("pos", "absolute"),
            ("inset", "0"),
            ("zIndex", "-1"),
            ("pointerEvents", "none"),
            ("overflow", "hidden"),
            ("borderRadius", "inherit"),
        ] {
            string_prop(&mut layer, name, value);
        }
        if let Some(blend) = paint["blendMode"].as_str().and_then(style::blend_mode) {
            string_prop(&mut layer, "mixBlendMode", blend);
        }
        let child = if paint["type"] == "IMAGE" && paint["scaleMode"] != "TILE" {
            let mapping = map_for(
                snapshot,
                node,
                Some(index),
                options.asset_policies,
                options.asset_names_per_node,
            );
            let mut image = Vec::new();
            string_prop(
                &mut image,
                "src",
                mapping["assetPath"].as_str().unwrap_or_default(),
            );
            string_prop(&mut image, "alt", "");
            apply_image_props(&mut image, &mapping);
            image.retain(|(n, _)| !matches!(n.as_str(), "aspectRatio" | "minW" | "minH"));
            for (name, value) in mapping["css"]["image"].as_object().into_iter().flatten() {
                if name == "opacity" {
                    continue;
                }
                if let Some(v) = value.as_str() {
                    string_prop(&mut image, name, v);
                }
            }
            for (name, value) in [
                ("pos", "absolute"),
                ("inset", "0"),
                ("w", "100%"),
                ("h", "100%"),
                ("maxW", "none"),
                ("display", "block"),
            ] {
                string_prop(&mut image, name, value);
            }
            if let Some(opacity) = paint["opacity"].as_f64()
                && opacity < 1.
            {
                string_prop(&mut layer, "opacity", number(opacity));
            }
            Some(element("Image", &image, None, depth + 1))
        } else {
            if let Some(css) = style::paint_css(snapshot, node, paint, index, false, used, options)
            {
                string_prop(&mut layer, "bg", css);
            }
            None
        };
        layers.push(element("Box", &layer, child.as_deref(), depth));
    }
    Some(layers)
}

/// Warn only for asset references and CSS on the same JSX element. Parsing
/// excludes comments/text and prevents unrelated ancestor effects matching.
pub fn validate_asset_effects(
    tsx: &str,
    policies: &BTreeMap<String, AssetRepresentationPolicy>,
) -> Vec<Diagnostic> {
    let attrs = crate::provenance::attributes::generated_attributes(tsx);
    let mut diagnostics = Vec::new();
    for (asset_id, policy) in policies {
        if policy.effects_baked != Some(true) {
            continue;
        }
        let paths = policy
            .asset_path
            .iter()
            .map(String::as_str)
            .chain(policy.variants.iter().map(|v| v.path.as_str()))
            .collect::<Vec<_>>();
        let elements = attrs
            .iter()
            .filter(|a| {
                matches!(
                    a.name.as_str(),
                    "src" | "bg" | "background" | "backgroundImage"
                ) && literal_attribute(&a.source).is_some_and(|v| {
                    paths.iter().any(|p| {
                        !p.is_empty() && (v == *p || background_urls(v).iter().any(|url| url == p))
                    })
                })
            })
            .map(|a| a.element)
            .collect::<BTreeSet<_>>();
        for element in elements {
            let repeated = attrs
                .iter()
                .filter(|a| {
                    a.element == element
                        && matches!(
                            a.name.as_str(),
                            "filter" | "boxShadow" | "textShadow" | "backdropFilter"
                        )
                        && literal_attribute(&a.source).is_some_and(|v| {
                            !matches!(v.trim(), "none" | "initial" | "inherit" | "unset" | "")
                        })
                })
                .map(|a| a.source.clone())
                .collect::<Vec<_>>();
            if !repeated.is_empty() {
                diagnostics.push(Diagnostic{code:"DEVUP_CODEGEN_DOUBLE_EFFECT".into(),message:"CSS effects on a baked asset create a double-effect risk; verify the effect kinds before applying them together.".into(),severity:Some(DiagnosticSeverity::Warning),details:Some(json!({"assetId":asset_id,"elementIndex":element,"effectsBaked":true,"generatedProperties":repeated})),..Diagnostic::default()});
            }
        }
    }
    diagnostics
}

fn literal_attribute(source: &str) -> Option<&str> {
    let (_, value) = source.split_once('=')?;
    let value = value.trim();
    let value = value
        .strip_prefix('{')
        .and_then(|v| v.strip_suffix('}'))
        .unwrap_or(value)
        .trim();
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .filter(|v| !v.contains(['"', '\\']))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|v| v.strip_suffix('\''))
                .filter(|v| !v.contains(['\'', '\\']))
        })
}
fn background_urls(value: &str) -> Vec<&str> {
    value
        .split("url(")
        .skip(1)
        .filter_map(|v| {
            v.split_once(')')
                .map(|(url, _)| url.trim().trim_matches(['\'', '"']))
        })
        .collect()
}

/// Variant/responsive trees use the same representation and paint geometry
/// as the ordinary renderer. Synthetic paint elements inherit provenance
/// from their outer source node rather than inventing Figma node identities.
pub(super) fn project_tree(
    mut tree: super::variant::Tree,
    snapshot: &Snapshot,
    node: &RawNode,
    options: &super::CodegenOptions,
) -> super::variant::Tree {
    if style::non_rendering_asset_reason(snapshot, node).is_some() {
        return tree;
    }
    use super::variant::Tree;
    let synthetic =
        |component: &str, props: BTreeMap<String, String>, children: Vec<Tree>, name: &str| Tree {
            node_id: String::new(),
            node_name: name.into(),
            source_node_ids: BTreeSet::new(),
            component: component.into(),
            props,
            children,
            content: None,
            is_component: false,
            visible_when: None,
            drawn_when: Vec::new(),
            leading_comment: None,
            slots: Vec::new(),
        };
    let visible = node.typed_view().value("fills").and_then(Value::as_array);
    if tree.component == "Image" {
        let index = visible.and_then(|fs| {
            fs.iter()
                .position(|p| p["type"] == "IMAGE" && p["visible"] != false)
        });
        if index.is_none() {
            return tree;
        }
        let mapping = map_for(
            snapshot,
            node,
            index,
            &options.asset_policies,
            options.asset_names_per_node,
        );
        if mapping["css"]["image"]["transform"].is_null() {
            return tree;
        }
        let image_names = [
            "src",
            "srcSet",
            "sizes",
            "width",
            "height",
            "objectFit",
            "objectPos",
            "alt",
        ];
        let mut image = tree
            .props
            .iter()
            .filter(|(n, _)| image_names.contains(&n.as_str()))
            .map(|(n, v)| (n.clone(), v.clone()))
            .collect::<BTreeMap<_, _>>();
        tree.props.retain(|n, _| !image_names.contains(&n.as_str()));
        tree.component = "Box".into();
        tree.props.entry("pos".into()).or_insert("relative".into());
        for (n, v) in mapping["css"]["image"].as_object().into_iter().flatten() {
            if let Some(v) = v.as_str() {
                image.insert(n.clone(), v.into());
            }
        }
        for (n, v) in [
            ("w", "100%"),
            ("h", "100%"),
            ("pos", "absolute"),
            ("left", "0"),
            ("top", "0"),
            ("maxW", "none"),
            ("display", "block"),
        ] {
            image.insert(n.into(), v.into());
        }
        let mut clip: BTreeMap<String, String> = [
            ("w", "100%"),
            ("h", "100%"),
            ("pos", "relative"),
            ("overflow", "hidden"),
            ("borderRadius", "inherit"),
        ]
        .into_iter()
        .map(|(n, v)| (n.into(), v.into()))
        .collect();
        for (dimension, css) in [("width", "w"), ("height", "h")] {
            if !tree.props.contains_key(css)
                && !tree.props.contains_key("boxSize")
                && let Some(value) = mapping["nodeBox"][dimension].as_f64()
            {
                clip.insert(css.into(), format!("{}px", number(value)));
            }
        }
        tree.children = vec![synthetic(
            "Box",
            clip,
            vec![synthetic("Image", image, vec![], "source-image")],
            "image-clip",
        )];
        return tree;
    }
    if tree.component == "Text" {
        return tree;
    }
    let Some(paints) = visible else {
        return tree;
    };
    if !paints.iter().enumerate().any(|(i, p)| {
        p["type"] == "IMAGE"
            && p["visible"] != false
            && policy_for(
                node,
                &format!("{}:fills:{i}", node.id),
                &options.asset_policies,
            )
            .original()
    }) {
        return tree;
    }
    let style_options = style::StyleOptions {
        reserve_image_content: !matches!(options.root_layout, super::RootLayout::Embedded),
        asset_policies: &options.asset_policies,
        variable_tokens: &options.variable_tokens,
        length_tokens: &options.length_tokens,
        asset_names_per_node: options.asset_names_per_node,
        width_policy: options.width_policy.as_ref(),
    };
    let mut layers = Vec::new();
    let mut used = BTreeSet::new();
    for (index, p) in paints
        .iter()
        .enumerate()
        .filter(|(_, p)| p["visible"] != false && p["opacity"] != 0)
    {
        let mut props: BTreeMap<String, String> = [
            ("pos", "absolute"),
            ("inset", "0"),
            ("zIndex", "-1"),
            ("pointerEvents", "none"),
            ("overflow", "hidden"),
            ("borderRadius", "inherit"),
        ]
        .into_iter()
        .map(|(n, v)| (n.into(), v.into()))
        .collect();
        if let Some(blend) = p["blendMode"].as_str().and_then(style::blend_mode) {
            props.insert("mixBlendMode".into(), blend);
        }
        let children = if p["type"] == "IMAGE" && p["scaleMode"] != "TILE" {
            let mapping = map_for(
                snapshot,
                node,
                Some(index),
                &options.asset_policies,
                options.asset_names_per_node,
            );
            let mut image_props = Vec::new();
            apply_image_props(&mut image_props, &mapping);
            let mut image = image_props
                .into_iter()
                .filter(|(n, _)| !matches!(n.as_str(), "aspectRatio" | "minW" | "minH"))
                .map(|(n, super::component::PropValue::String(v))| (n, v))
                .collect::<BTreeMap<_, _>>();
            image.insert(
                "src".into(),
                mapping["assetPath"].as_str().unwrap_or_default().into(),
            );
            image.insert("alt".into(), String::new());
            for (n, v) in mapping["css"]["image"].as_object().into_iter().flatten() {
                if n == "opacity" {
                    continue;
                }
                if let Some(v) = v.as_str() {
                    image.insert(n.clone(), v.into());
                }
            }
            for (n, v) in [
                ("pos", "absolute"),
                ("inset", "0"),
                ("w", "100%"),
                ("h", "100%"),
                ("maxW", "none"),
                ("display", "block"),
            ] {
                image.insert(n.into(), v.into());
            }
            if let Some(opacity) = p["opacity"].as_f64()
                && opacity < 1.
            {
                props.insert("opacity".into(), number(opacity));
            }
            vec![synthetic(
                "Image",
                image,
                vec![],
                &format!("paint-{index}-image"),
            )]
        } else {
            if let Some(css) =
                style::paint_css(snapshot, node, p, index, false, &mut used, style_options)
            {
                props.insert("bg".into(), css);
            }
            vec![]
        };
        layers.push(synthetic("Box", props, children, &format!("paint-{index}")));
    }
    tree.props
        .retain(|n, _| !matches!(n.as_str(), "bg" | "bgBlendMode" | "objectFit" | "objectPos"));
    tree.props.entry("pos".into()).or_insert("relative".into());
    tree.props.insert("isolation".into(), "isolate".into());
    tree.children.splice(0..0, layers);
    tree
}

pub(super) fn map_for(
    snapshot: &Snapshot,
    node: &RawNode,
    index: Option<usize>,
    policies: &BTreeMap<String, AssetRepresentationPolicy>,
    per_node: bool,
) -> Value {
    let id = index
        .map(|i| format!("{}:fills:{i}", node.id))
        .unwrap_or_else(|| node.id.clone());
    let default_path = index
        .and_then(|i| style::image_fill_path(snapshot, &node.id, i, per_node))
        .or_else(|| style::asset_path(snapshot, &node.id, per_node))
        .unwrap_or_default();
    asset_css_map(
        &serde_json::to_value(node).expect("RawNode serializes"),
        &id,
        &default_path,
        &policy_for(node, &id, policies),
    )
}

pub(super) fn attach_evidence(
    snapshot: &Snapshot,
    options: &super::CodegenOptions,
    output: &mut super::CodegenOutput,
) {
    use crate::provenance::{GeneratedRange, ProvenanceEntry};
    let attrs = crate::provenance::attributes::generated_attributes(&output.tsx);
    let emitted = output
        .source_map
        .entries
        .iter()
        .filter(|e| e.property.is_none() && e.resolution == "node")
        .filter_map(|e| Some((e.node_id.clone()?, e.generated_range.clone()?)))
        .collect::<Vec<_>>();
    for (id, range) in emitted {
        let Some(node) = snapshot.nodes.get(&id) else {
            continue;
        };
        let indices = node
            .typed_view()
            .value("fills")
            .and_then(Value::as_array)
            .map(|fs| {
                fs.iter()
                    .enumerate()
                    .filter(|(_, p)| p["type"] == "IMAGE" && p["visible"] != false)
                    .map(|(i, _)| Some(i))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let indices = if indices.is_empty()
            && style::asset_kind(snapshot, node) == Some(style::AssetKind::Png)
        {
            vec![None]
        } else {
            indices
        };
        for index in indices {
            let mut mapping = map_for(
                snapshot,
                node,
                index,
                &options.asset_policies,
                options.asset_names_per_node,
            );
            let path = mapping["assetPath"].as_str().unwrap_or_default().to_owned();
            let asset_id = mapping["assetId"].as_str().unwrap_or(&id).to_owned();
            // Nested nodes have their own range; only this node's properties
            // can substantiate its mapping, even if another instance reuses bytes.
            let own = attrs
                .iter()
                .filter(|a| range.start <= a.start && a.end <= range.end)
                .filter(|a| {
                    !output.source_map.entries.iter().any(|e| {
                        e.property.is_none()
                            && e.node_id.as_deref() != Some(&id)
                            && e.generated_range.as_ref().is_some_and(|r| {
                                range.start <= r.start
                                    && r.end <= range.end
                                    && r.start <= a.start
                                    && a.end <= r.end
                            })
                    })
                })
                .collect::<Vec<_>>();
            let references = own
                .iter()
                .filter(|a| {
                    matches!(a.name.as_str(), "src" | "bg")
                        && !path.is_empty()
                        && literal_attribute(&a.source).is_some_and(|value| {
                            value == path || background_urls(value).iter().any(|url| *url == path)
                        })
                })
                .collect::<Vec<_>>();
            if references.is_empty() {
                let policy = policy_for(node, &asset_id, &options.asset_policies);
                if !policy.incompatible_capture(&id) {
                    continue;
                }
                mapping["emitted"] = json!(false);
                mapping["effectsAppliedBy"] = Value::Null;
                mapping["emittedProperties"] = json!([]);
                mapping["propertyEvidence"] = json!([]);
                output.diagnostics.push(Diagnostic{code:"DEVUP_CODEGEN_IMAGE_MAPPING_UNSUPPORTED".into(),message:"A visible capture of a different node cannot be composed into this leaf's layout box.".into(),node_id:Some(id.clone()),property:Some("fills".into()),fidelity_impact:Some(devup_mcp_figma::FidelityImpact::Lossy),details:Some(json!({"assetId":asset_id,"reason":"incompatible-visible-capture","captureNodeId":policy.capture_node_id,"captureBox":policy.capture_box,"emittedAssetReference":false})),..Diagnostic::default()});
                output.source_map.asset_css_mappings.push(mapping);
                continue;
            }
            mapping["emitted"] = json!(true);
            let properties=own.iter().map(|a|json!({"elementIndex":a.element,"name":a.name,"generatedProperty":a.source})).collect::<Vec<_>>();
            mapping["emittedProperties"] = json!(properties);
            // The public source map exposes verified emission, while the
            // reusable recipe remains useful before code generation.
            mapping["propertyEvidence"]=json!(own.iter().filter_map(|a| {
                let source=match a.name.as_str(){"filter"|"boxShadow"|"backdropFilter"=>"effects","borderRadius"|"borderTopLeftRadius"|"borderTopRightRadius"|"borderBottomLeftRadius"|"borderBottomRightRadius"=>"radii","width"|"height"=>"acquired-pixel-dimensions","transform"|"transformOrigin"|"objectFit"|"objectPos"=>"image-paint-geometry","srcSet"|"sizes"=>"acquired-variants","opacity"=>"node-or-paint-opacity","mixBlendMode"|"bgBlendMode"=>"node-or-paint-blend-mode","bg"=>"paint-stack-gradient-or-solid",_=>return None};
                Some(json!({"property":a.name,"generatedProperty":a.source,"source":source,"elementIndex":a.element,"scope":"node-paint-stack","effectsAppliedBy":mapping["effectsAppliedBy"]}))
            }).collect::<Vec<_>>());
            for a in own.iter().filter(|a| {
                matches!(
                    a.name.as_str(),
                    "width"
                        | "height"
                        | "srcSet"
                        | "sizes"
                        | "objectFit"
                        | "objectPos"
                        | "transform"
                        | "transformOrigin"
                        | "aspectRatio"
                        | "minW"
                        | "minH"
                        | "filter"
                        | "boxShadow"
                        | "backdropFilter"
                )
            }) {
                let property = match a.name.as_str() {
                    "width" | "height" => "sourcePixelSize",
                    "srcSet" | "sizes" => "assetVariants",
                    "filter" | "boxShadow" | "backdropFilter" => "effects",
                    _ => "fills",
                };
                output.source_map.entries.push(ProvenanceEntry {generated_range:Some(GeneratedRange{start:a.start,end:a.end}),calculation:Some("Acquisition representation policy and image-paint geometry; see assetCssMappings for dimensions, coordinate semantics and limits.".into()),generated_property:Some(a.source.clone()),json_pointer:None,node_id:Some(id.clone()),property:Some(property.into()),variable_id:None,style_id:None,asset_id:Some(asset_id.clone()),resolution:"asset-css-projection".into(),element_index:Some(a.element)});
            }
            for a in &references {
                output.source_map.entries.push(ProvenanceEntry {
                    generated_range: Some(GeneratedRange {
                        start: a.start,
                        end: a.end,
                    }),
                    calculation: Some("Instance-specific acquired image path.".into()),
                    generated_property: Some(a.source.clone()),
                    json_pointer: None,
                    node_id: Some(id.clone()),
                    property: Some("fills".into()),
                    variable_id: None,
                    style_id: None,
                    asset_id: Some(asset_id.clone()),
                    resolution: "asset".into(),
                    element_index: Some(a.element),
                });
                if mapping["effectsBaked"] == true {
                    let mut entry = output.source_map.entries.last().unwrap().clone();
                    entry.property = Some("effects".into());
                    entry.resolution = "effects-baked-into-asset".into();
                    entry.calculation=Some("Supplied acquisition evidence confirms baked effects; the same CSS effects are suppressed.".into());
                    output.source_map.entries.push(entry);
                }
            }
            for reason in mapping["warnings"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                let unsupported = mapping["unsupported"]
                    .as_array()
                    .is_some_and(|xs| xs.iter().any(|x| x == reason));
                output.diagnostics.push(Diagnostic{code:if unsupported{"DEVUP_CODEGEN_IMAGE_MAPPING_UNSUPPORTED"}else{"DEVUP_CODEGEN_IMAGE_MAPPING_EVIDENCE"}.into(),message:format!("Image mapping: {reason}."),node_id:Some(id.clone()),property:Some("fills".into()),severity:Some(DiagnosticSeverity::Warning),fidelity_impact:Some(if unsupported{devup_mcp_figma::FidelityImpact::Lossy}else{devup_mcp_figma::FidelityImpact::Approximated}),details:Some(json!({"assetId":asset_id,"reason":reason,"renderMode":mapping["renderMode"],"evidenceLimit":mapping["coordinateSemantics"]["scope"]})),..Diagnostic::default()});
            }
            output.source_map.asset_css_mappings.push(mapping);
        }
    }
    output
        .diagnostics
        .extend(validate_asset_effects(&output.tsx, &options.asset_policies));
}
