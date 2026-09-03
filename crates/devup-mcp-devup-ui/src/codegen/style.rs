use std::collections::BTreeSet;

use devup_mcp_figma::{RawNode, Snapshot, TypedNode};
use serde_json::Value;

use super::{
    component::Prop,
    layout::{format_number, px, string_prop},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AssetKind {
    Svg,
    SvgMask,
    Png,
}

pub(super) fn asset_kind(snapshot: &Snapshot, node: &RawNode) -> Option<AssetKind> {
    let view = node.typed_view();
    if matches!(view.node_type(), "TEXT" | "COMPONENT_SET") {
        return None;
    }
    if view
        .value("inferredAutoLayout")
        .and_then(Value::as_object)
        .and_then(|layout| layout.get("layoutMode"))
        .and_then(Value::as_str)
        == Some("GRID")
    {
        return None;
    }
    if matches!(view.node_type(), "VECTOR" | "STAR" | "POLYGON")
        || (view.node_type() == "ELLIPSE"
            && view
                .value("arcData")
                .and_then(|value| value.get("innerRadius"))
                .and_then(Value::as_f64)
                .is_some_and(|value| value != 0.0))
    {
        return Some(if uniform_asset_color(snapshot, node).is_some() {
            AssetKind::SvgMask
        } else {
            AssetKind::Svg
        });
    }
    let fills = view.value("fills").and_then(Value::as_array);
    if view.bool("isAsset") == Some(true) {
        if fills.is_some_and(|fills| {
            fills.len() == 1
                && fills[0].get("type").and_then(Value::as_str) == Some("IMAGE")
                && fills[0].get("scaleMode").and_then(Value::as_str) != Some("TILE")
        }) {
            return Some(AssetKind::Png);
        }
        if fills.is_some_and(|fills| {
            !fills.is_empty()
                && !fills.iter().all(|paint| {
                    paint.get("type").and_then(Value::as_str) == Some("SOLID")
                        && paint.get("visible").and_then(Value::as_bool) == Some(true)
                })
        }) {
            return Some(if uniform_asset_color(snapshot, node).is_some() {
                AssetKind::SvgMask
            } else {
                AssetKind::Svg
            });
        }
    }
    if view.child_ids().next().is_some() {
        let children = view
            .child_ids()
            .filter_map(|id| snapshot.nodes.get(id))
            .collect::<Vec<_>>();
        let direct_vectors = children.iter().all(|child| {
            matches!(
                child.typed_view().node_type(),
                "VECTOR" | "STAR" | "POLYGON"
            )
        });
        if view.bool("isAsset") == Some(true) && first_solid_color(view.value("fills")).is_some() {
            return None;
        }
        if children.len() == 1
            && !direct_vectors
            && matches!(
                view.string("layoutMode"),
                Some("HORIZONTAL" | "VERTICAL" | "GRID")
            )
        {
            return None;
        }
        if !children.is_empty()
            && children.iter().all(|child| {
                matches!(
                    asset_kind_nested(snapshot, child),
                    Some(AssetKind::Svg | AssetKind::SvgMask)
                )
            })
        {
            return Some(if uniform_asset_color(snapshot, node).is_some() {
                AssetKind::SvgMask
            } else {
                AssetKind::Svg
            });
        }
    }
    None
}

fn asset_kind_nested(snapshot: &Snapshot, node: &RawNode) -> Option<AssetKind> {
    if let Some(kind) = asset_kind(snapshot, node) {
        return Some(kind);
    }
    let view = node.typed_view();
    if view.node_type() == "TEXT" {
        return None;
    }
    if view.child_ids().next().is_some() {
        return None;
    }
    let fills = view.value("fills").and_then(Value::as_array)?;
    if fills.iter().any(|paint| {
        paint.get("visible").and_then(Value::as_bool) != Some(false)
            && paint.get("type").and_then(Value::as_str) != Some("SOLID")
    }) {
        return None;
    }
    if fills.iter().any(|paint| {
        paint.get("visible").and_then(Value::as_bool) != Some(false)
            && matches!(
                paint.get("type").and_then(Value::as_str),
                Some("IMAGE" | "VIDEO" | "PATTERN")
            )
    }) {
        None
    } else {
        Some(if uniform_asset_color(snapshot, node).is_some() {
            AssetKind::SvgMask
        } else {
            AssetKind::Svg
        })
    }
}

fn uniform_asset_color(snapshot: &Snapshot, node: &RawNode) -> Option<String> {
    fn visit(snapshot: &Snapshot, node: &RawNode, colors: &mut Vec<String>) -> bool {
        let view = node.typed_view();
        for field in ["fills", "strokes"] {
            if let Some(paints) = view.value(field).and_then(Value::as_array) {
                for paint in paints {
                    if paint.get("visible").and_then(Value::as_bool) == Some(false) {
                        continue;
                    }
                    if paint.get("type").and_then(Value::as_str) != Some("SOLID") {
                        return false;
                    }
                    // Must go through `color_from_paint`, not `color_from` on the
                    // raw `color`: Figma splits a translucent solid across
                    // `color.a` and the paint's own `opacity`, and the effective
                    // alpha is the product. Formatting `color` alone silently
                    // drops `opacity` and renders the asset fully opaque, which
                    // also made this path disagree with `first_solid_color` on
                    // byte-identical input.
                    let Some(color) = color_from_paint(paint) else {
                        return false;
                    };
                    colors.push(color);
                }
            }
        }
        view.child_ids()
            .filter_map(|id| snapshot.nodes.get(id))
            .all(|child| visit(snapshot, child, colors))
    }

    let mut colors = Vec::new();
    if !visit(snapshot, node, &mut colors) || colors.is_empty() {
        return None;
    }
    let first = colors.first()?.clone();
    colors.iter().all(|color| color == &first).then_some(first)
}

fn uniform_asset_token(
    snapshot: &Snapshot,
    node: &RawNode,
    variable_tokens: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    fn visit(
        snapshot: &Snapshot,
        node: &RawNode,
        variable_tokens: &std::collections::BTreeMap<String, String>,
        tokens: &mut Vec<String>,
    ) -> bool {
        let view = node.typed_view();
        if let Some(fills) = view.value("fills").and_then(Value::as_array) {
            for paint in fills.iter().filter(|paint| {
                paint.get("visible").and_then(Value::as_bool) != Some(false)
                    && paint.get("type").and_then(Value::as_str) == Some("SOLID")
            }) {
                let Some(token) = bound_paint_token(paint, variable_tokens) else {
                    return false;
                };
                tokens.push(token);
            }
        }
        view.child_ids()
            .filter_map(|id| snapshot.nodes.get(id))
            .all(|child| visit(snapshot, child, variable_tokens, tokens))
    }

    let mut tokens = Vec::new();
    if !visit(snapshot, node, variable_tokens, &mut tokens) || tokens.is_empty() {
        return None;
    }
    let first = tokens.first()?.clone();
    tokens.iter().all(|token| token == &first).then_some(first)
}

pub(super) fn push_style_props(
    snapshot: &Snapshot,
    node: &RawNode,
    component: &str,
    asset: Option<AssetKind>,
    props: &mut Vec<Prop>,
    used_tokens: &mut BTreeSet<String>,
    variable_tokens: &std::collections::BTreeMap<String, String>,
) {
    let view = node.typed_view();
    if view.bool("visible") == Some(false) {
        string_prop(props, "display", "none");
    }
    if let Some(asset) = asset {
        let folder = if matches!(asset, AssetKind::Svg | AssetKind::SvgMask) {
            "icons"
        } else {
            "images"
        };
        let extension = if matches!(asset, AssetKind::Svg | AssetKind::SvgMask) {
            "svg"
        } else {
            "png"
        };
        let source = format!("/{folder}/{}.{extension}", view.name().unwrap_or("Asset"));
        if asset == AssetKind::SvgMask {
            if let Some(token) = uniform_asset_token(snapshot, node, variable_tokens) {
                used_tokens.insert(token.clone());
                string_prop(props, "bg", format!("${token}"));
            } else if let Some(color) = uniform_asset_color(snapshot, node) {
                string_prop(props, "bg", color);
            }
            let url = if source.contains(' ') {
                format!("url('{source}')")
            } else {
                format!("url({source})")
            };
            string_prop(props, "maskImage", url);
            string_prop(props, "maskRepeat", "no-repeat");
            string_prop(props, "maskSize", "contain");
            string_prop(props, "maskPos", "center");
        } else {
            string_prop(props, "src", source);
        }
        if asset == AssetKind::Png
            && let Some(scale) = view
                .value("fills")
                .and_then(Value::as_array)
                .and_then(|fills| fills.first())
                .and_then(|paint| paint.get("scaleMode"))
                .and_then(Value::as_str)
        {
            match scale {
                "FIT" => string_prop(props, "objectFit", "contain"),
                "CROP" => string_prop(props, "objectFit", "cover"),
                _ => {}
            }
        }
        push_radius(&view, props);
        push_strokes(&view, props, used_tokens, variable_tokens);
        push_effects(&view, component, props);
        if let Some(opacity) = view.number("opacity")
            && opacity < 1.0
        {
            string_prop(props, "opacity", format_number(opacity));
        }
        push_blend_mode(&view, props);
        return;
    }

    let color_prop = if component == "Text" { "color" } else { "bg" };
    if let Some(token) = view
        .value("devupTokens")
        .and_then(Value::as_object)
        .and_then(|tokens| tokens.get("fills"))
        .and_then(Value::as_str)
    {
        used_tokens.insert(token.to_owned());
        string_prop(props, color_prop, format!("${token}"));
    } else if let Some(color) = bound_color_token(&view, variable_tokens) {
        used_tokens.insert(color.clone());
        string_prop(props, color_prop, format!("${color}"));
    } else if component == "Text" && has_non_solid_fill(&view) {
        if let Some(background) = background_css(snapshot, node, variable_tokens) {
            string_prop(props, "bg", background);
            string_prop(props, "bgClip", "text");
            string_prop(props, "WebkitTextFillColor", "transparent");
        }
    } else if component == "Text" {
        let transparent = view
            .value("fills")
            .and_then(Value::as_array)
            .is_some_and(|fills| {
                fills.iter().any(|paint| {
                    paint.get("type").and_then(Value::as_str) == Some("SOLID")
                        && paint.get("visible").and_then(Value::as_bool) != Some(false)
                        && paint.get("opacity").and_then(Value::as_f64) == Some(0.0)
                })
            });
        if transparent {
            string_prop(props, color_prop, "transparent");
        } else if let Some(color) = first_solid_color(view.value("fills")) {
            string_prop(props, color_prop, color);
        }
    } else if let Some(background) = background_css(snapshot, node, variable_tokens) {
        string_prop(props, "bg", background);
    }
    if let Some(mode) = view
        .value("fills")
        .and_then(Value::as_array)
        .and_then(|fills| {
            fills.iter().rev().find_map(|paint| {
                (paint.get("visible").and_then(Value::as_bool) != Some(false))
                    .then(|| paint.get("blendMode").and_then(Value::as_str))
                    .flatten()
                    .and_then(blend_mode)
            })
        })
    {
        string_prop(props, "bgBlendMode", mode);
    }

    push_radius(&view, props);
    if component != "Text" {
        push_strokes(&view, props, used_tokens, variable_tokens);
    }
    push_effects(&view, component, props);
    if let Some(opacity) = view.number("opacity")
        && opacity < 1.0
    {
        string_prop(props, "opacity", format_number(opacity));
    }
    push_blend_mode(&view, props);
}

fn push_blend_mode(view: &TypedNode<'_>, props: &mut Vec<Prop>) {
    let Some(blend) = view.string("blendMode") else {
        return;
    };
    let css = match blend {
        "PASS_THROUGH" | "NORMAL" => None,
        "LINEAR_BURN" => Some("linearBurn".to_owned()),
        "COLOR_BURN" => Some("colorBurn".to_owned()),
        "LINEAR_DODGE" => Some("linear-dodge".to_owned()),
        "COLOR_DODGE" => Some("color-dodge".to_owned()),
        "SOFT_LIGHT" => Some("soft-light".to_owned()),
        "HARD_LIGHT" => Some("hard-light".to_owned()),
        value => Some(value.to_ascii_lowercase()),
    };
    if let Some(css) = css {
        string_prop(props, "mixBlendMode", css);
    }
}

fn has_non_solid_fill(view: &TypedNode<'_>) -> bool {
    view.value("fills")
        .and_then(Value::as_array)
        .is_some_and(|fills| {
            fills.iter().any(|paint| {
                paint.get("visible").and_then(Value::as_bool) != Some(false)
                    && paint.get("opacity").and_then(Value::as_f64) != Some(0.0)
                    && paint.get("type").and_then(Value::as_str) != Some("SOLID")
            })
        })
}

fn background_css(
    snapshot: &Snapshot,
    node: &RawNode,
    variable_tokens: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    let view = node.typed_view();
    let paints = view.value("fills")?.as_array()?;
    let visible = paints
        .iter()
        .filter(|paint| {
            paint.get("visible").and_then(Value::as_bool) != Some(false)
                && paint.get("opacity").and_then(Value::as_f64) != Some(0.0)
        })
        .rev()
        .collect::<Vec<_>>();
    let mut css = Vec::new();
    for (index, paint) in visible.iter().enumerate() {
        let is_last = index + 1 == visible.len();
        if let Some(value) = paint_css(snapshot, node, paint, is_last, variable_tokens) {
            css.push(value);
        }
    }
    (!css.is_empty()).then(|| css.join(", "))
}

fn paint_css(
    snapshot: &Snapshot,
    node: &RawNode,
    paint: &Value,
    last: bool,
    variable_tokens: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    let kind = paint.get("type")?.as_str()?;
    match kind {
        "SOLID" => {
            let color = bound_paint_token(paint, variable_tokens)
                .map(|token| format!("${token}"))
                .or_else(|| color_from_paint(paint))?;
            Some(if last {
                color
            } else {
                format!("linear-gradient({color}, {color})")
            })
        }
        "GRADIENT_LINEAR" => gradient_css(node, paint, "linear", variable_tokens),
        "GRADIENT_RADIAL" => gradient_css(node, paint, "radial", variable_tokens),
        "GRADIENT_ANGULAR" => gradient_css(node, paint, "angular", variable_tokens),
        "GRADIENT_DIAMOND" => gradient_css(node, paint, "diamond", variable_tokens),
        "IMAGE" => {
            let fit = match paint.get("scaleMode").and_then(Value::as_str) {
                Some("FIT") => "center/contain no-repeat",
                Some("FILL" | "CROP") => "center/cover no-repeat",
                Some("TILE") => "repeat",
                _ => "center/cover no-repeat",
            };
            Some(format!("url(/icons/image.png) {fit}"))
        }
        "PATTERN" => {
            let source_id = paint.get("sourceNodeId").and_then(Value::as_str)?;
            let source = snapshot.nodes.get(source_id);
            let name = source
                .and_then(|node| node.typed_view().name())
                .unwrap_or("pattern");
            let extension = source
                .and_then(|node| asset_kind(snapshot, node))
                .map(|kind| if kind == AssetKind::Png { "png" } else { "svg" })
                .unwrap_or("svg");
            let spacing = paint.get("spacing").and_then(Value::as_object);
            let x = spacing
                .and_then(|value| value.get("x"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let y = spacing
                .and_then(|value| value.get("y"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let horizontal = position_part(
                paint
                    .get("horizontalAlignment")
                    .and_then(Value::as_str)
                    .unwrap_or("START"),
                x,
                ["left", "center", "right"],
            );
            let vertical = position_part(
                paint
                    .get("verticalAlignment")
                    .and_then(Value::as_str)
                    .unwrap_or("START"),
                y,
                ["top", "center", "bottom"],
            );
            let position = [horizontal, vertical]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            Some(format!(
                "url(/icons/{name}.{extension}){} repeat",
                if position.is_empty() {
                    String::new()
                } else {
                    format!(" {position}")
                }
            ))
        }
        _ => None,
    }
}

fn position_part(alignment: &str, spacing: f64, values: [&str; 3]) -> Option<String> {
    if alignment == "START" && spacing == 0.0 {
        return None;
    }
    let value = match alignment {
        "CENTER" => values[1],
        "END" => values[2],
        _ => values[0],
    };
    Some(format!("{value} {}%", format_number(spacing * 100.0)))
}

fn gradient_css(
    node: &RawNode,
    paint: &Value,
    kind: &str,
    variable_tokens: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    let opacity = paint.get("opacity").and_then(Value::as_f64).unwrap_or(1.0);
    let raw_stops = paint
        .get("gradientStops")?
        .as_array()?
        .iter()
        .filter_map(|stop| {
            let mut color = stop.get("color")?.clone();
            let alpha = color.get("a").and_then(Value::as_f64).unwrap_or(1.0) * opacity;
            color
                .as_object_mut()?
                .insert("a".to_owned(), Value::from(alpha));
            let color = bound_paint_token(stop, variable_tokens)
                .map(|token| format!("${token}"))
                .or_else(|| color_from(&color))?;
            Some((stop.get("position")?.as_f64()?, color))
        })
        .collect::<Vec<_>>();
    let transform = paint
        .get("gradientTransform")
        .and_then(parse_transform)
        .unwrap_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
    let view = node.typed_view();
    let width = view.number("width").unwrap_or(1.0);
    let height = view.number("height").unwrap_or(1.0);
    let (prefix, positions) = match kind {
        "linear" => {
            let (angle, positions) = linear_geometry(transform, width, height, &raw_stops)?;
            (
                format!("linear-gradient({}deg, ", format_number(angle)),
                positions,
            )
        }
        "radial" => {
            let inverse = inverse_transform(transform)?;
            let center = apply_transform(inverse, [0.5, 0.5]);
            let radius = apply_transform(inverse, [1.0, 1.0]);
            (
                format!(
                    "radial-gradient({}% {}% at {}% {}%, ",
                    format_number((radius[0] - center[0]).abs() * 100.0),
                    format_number((radius[1] - center[1]).abs() * 100.0),
                    format_number(center[0] * 100.0),
                    format_number(center[1] * 100.0)
                ),
                raw_stops
                    .iter()
                    .map(|(position, _)| position * 100.0)
                    .collect(),
            )
        }
        "angular" => {
            let inverse = inverse_transform(transform)?;
            let center = apply_transform(inverse, [0.5, 0.5]);
            let start = apply_transform(inverse, [1.0, 0.5]);
            let mut angle = ((start[1] - center[1]) * height)
                .atan2((start[0] - center[0]) * width)
                .to_degrees()
                + 90.0;
            angle %= 360.0;
            if angle < 0.0 {
                angle += 360.0;
            }
            (
                format!(
                    "conic-gradient(from {}deg at {}% {}%, ",
                    format_number(angle),
                    format_number(center[0] * 100.0),
                    format_number(center[1] * 100.0)
                ),
                raw_stops
                    .iter()
                    .map(|(position, _)| position * 100.0)
                    .collect(),
            )
        }
        "diamond" => (
            String::new(),
            raw_stops
                .iter()
                .map(|(position, _)| position * 50.0)
                .collect(),
        ),
        _ => return None,
    };
    let stops = raw_stops
        .iter()
        .zip(positions)
        .map(|((_, color), position)| format!("{color} {}%", format_number(position)))
        .collect::<Vec<_>>()
        .join(", ");
    Some(match kind {
        "linear" | "radial" | "angular" => format!("{prefix}{stops})"),
        "diamond" => [
            "bottom right|to bottom right",
            "bottom left|to bottom left",
            "top left|to top left",
            "top right|to top right",
        ]
        .map(|entry| {
            let (position, direction) = entry.split_once('|').expect("diamond direction");
            format!("linear-gradient({direction}, {stops}) {position} / 50.1% 50.1% no-repeat")
        })
        .join(", "),
        _ => return None,
    })
}

fn parse_transform(value: &Value) -> Option<[[f64; 3]; 2]> {
    let rows = value.as_array()?;
    let row = |index: usize| -> Option<[f64; 3]> {
        let values = rows.get(index)?.as_array()?;
        Some([
            values.first()?.as_f64()?,
            values.get(1)?.as_f64()?,
            values.get(2)?.as_f64()?,
        ])
    };
    Some([row(0)?, row(1)?])
}

fn inverse_transform(matrix: [[f64; 3]; 2]) -> Option<[[f64; 3]; 2]> {
    let [[a, b, c], [d, e, f]] = matrix;
    let determinant = a * e - b * d;
    (determinant.abs() > f64::EPSILON).then_some([
        [
            e / determinant,
            -b / determinant,
            (b * f - c * e) / determinant,
        ],
        [
            -d / determinant,
            a / determinant,
            (c * d - a * f) / determinant,
        ],
    ])
}

fn apply_transform(matrix: [[f64; 3]; 2], point: [f64; 2]) -> [f64; 2] {
    [
        matrix[0][0] * point[0] + matrix[0][1] * point[1] + matrix[0][2],
        matrix[1][0] * point[0] + matrix[1][1] * point[1] + matrix[1][2],
    ]
}

fn linear_geometry(
    transform: [[f64; 3]; 2],
    width: f64,
    height: f64,
    stops: &[(f64, String)],
) -> Option<(f64, Vec<f64>)> {
    let inverse = inverse_transform(transform)?;
    let normalized_start = apply_transform(inverse, [0.0, 0.5]);
    let normalized_end = apply_transform(inverse, [1.0, 0.5]);
    let start = [normalized_start[0] * width, normalized_start[1] * height];
    let end = [normalized_end[0] * width, normalized_end[1] * height];
    let mut figma_angle = (end[1] - start[1]).atan2(end[0] - start[0]).to_degrees() - 90.0;
    figma_angle %= 360.0;
    if figma_angle < 0.0 {
        figma_angle += 360.0;
    }
    let angle = ((figma_angle - 180.0) % 360.0).round();
    let radians = angle.to_radians();
    let half = ((width * radians.sin()).abs() + (height * radians.cos()).abs()) / 2.0;
    let center = [width / 2.0, height / 2.0];
    let css_radians = (angle - 90.0).to_radians();
    let css_start = [
        center[0] - half * css_radians.cos(),
        center[1] - half * css_radians.sin(),
    ];
    let css_end = [
        center[0] + half * css_radians.cos(),
        center[1] + half * css_radians.sin(),
    ];
    let figma_vector = [end[0] - start[0], end[1] - start[1]];
    let css_vector = [css_end[0] - css_start[0], css_end[1] - css_start[1]];
    let denominator = css_vector[0].powi(2) + css_vector[1].powi(2);
    let positions = stops
        .iter()
        .map(|(position, _)| {
            let point = [
                start[0] + figma_vector[0] * position,
                start[1] + figma_vector[1] * position,
            ];
            let relative = [point[0] - css_start[0], point[1] - css_start[1]];
            if denominator == 0.0 {
                0.0
            } else {
                (relative[0] * css_vector[0] + relative[1] * css_vector[1]) / denominator * 100.0
            }
        })
        .collect();
    Some((angle, positions))
}

fn color_from_paint(paint: &Value) -> Option<String> {
    let mut color = paint.get("color")?.clone();
    let alpha = color.get("a").and_then(Value::as_f64).unwrap_or(1.0)
        * paint.get("opacity").and_then(Value::as_f64).unwrap_or(1.0);
    color
        .as_object_mut()?
        .insert("a".to_owned(), Value::from(alpha));
    color_from(&color)
}

fn blend_mode(mode: &str) -> Option<String> {
    match mode {
        "PASS_THROUGH" | "NORMAL" => None,
        "LINEAR_BURN" => Some("linearBurn".to_owned()),
        "COLOR_BURN" => Some("colorBurn".to_owned()),
        "LINEAR_DODGE" => Some("linear-dodge".to_owned()),
        "COLOR_DODGE" => Some("color-dodge".to_owned()),
        "SOFT_LIGHT" => Some("soft-light".to_owned()),
        "HARD_LIGHT" => Some("hard-light".to_owned()),
        value => Some(value.to_ascii_lowercase()),
    }
}

fn bound_color_token(
    view: &TypedNode<'_>,
    variable_tokens: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    let id = view.value("fills")?.as_array()?.iter().find_map(|paint| {
        paint
            .get("boundVariables")?
            .get("color")?
            .get("id")?
            .as_str()
    })?;
    variable_tokens
        .get(id)
        .cloned()
        .or_else(|| (id == "var1").then(|| "primaryColor".to_owned()))
        .or_else(|| Some(id.split([':', '/']).next_back().unwrap_or(id).to_owned()))
}

fn bound_paint_token(
    paint: &Value,
    variable_tokens: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    let id = paint
        .get("boundVariables")?
        .get("color")?
        .get("id")?
        .as_str()?;
    variable_tokens.get(id).cloned()
}

fn push_radius(view: &TypedNode<'_>, props: &mut Vec<Prop>) {
    if view.node_type() == "ELLIPSE"
        && view
            .value("arcData")
            .and_then(|value| value.get("innerRadius"))
            .and_then(Value::as_f64)
            == Some(0.0)
    {
        string_prop(props, "borderRadius", "50%");
        return;
    }
    if let Some(radius) = view.number("cornerRadius")
        && radius != 0.0
    {
        string_prop(props, "borderRadius", px(radius));
        return;
    }
    let radii = [
        view.number("topLeftRadius"),
        view.number("topRightRadius"),
        view.number("bottomRightRadius"),
        view.number("bottomLeftRadius"),
    ];
    if let [Some(a), Some(b), Some(c), Some(d)] = radii {
        if a == 0.0 && b == 0.0 && c == 0.0 && d == 0.0 {
            return;
        }
        let value = if a == b && b == c && c == d {
            px(a)
        } else if a == c && b == d {
            format!("{} {}", px(a), px(b))
        } else if b == d {
            format!("{} {} {}", px(a), px(b), px(c))
        } else {
            [a, b, c, d].map(px).join(" ")
        };
        string_prop(props, "borderRadius", value);
    }
}

fn push_strokes(
    view: &TypedNode<'_>,
    props: &mut Vec<Prop>,
    used_tokens: &mut BTreeSet<String>,
    variable_tokens: &std::collections::BTreeMap<String, String>,
) {
    let Some(paint) = view
        .value("strokes")
        .and_then(Value::as_array)
        .and_then(|strokes| {
            strokes.iter().find(|paint| {
                paint.get("visible").and_then(Value::as_bool) != Some(false)
                    && paint.get("type").and_then(Value::as_str) == Some("SOLID")
            })
        })
    else {
        return;
    };
    let color = if let Some(token) = bound_paint_token(paint, variable_tokens) {
        used_tokens.insert(token.clone());
        format!("${token}")
    } else if let Some(color) = color_from_paint(paint) {
        color
    } else {
        return;
    };
    let style = if view
        .value("dashPattern")
        .and_then(Value::as_array)
        .is_some_and(|value| !value.is_empty())
    {
        "dashed"
    } else {
        "solid"
    };
    let align = view.string("strokeAlign").unwrap_or("INSIDE");
    let explicit_weight = view.number("strokeWeight");
    if explicit_weight.is_none() {
        let sides = [
            ("strokeTopWeight", "borderTop"),
            ("strokeRightWeight", "borderRight"),
            ("strokeBottomWeight", "borderBottom"),
            ("strokeLeftWeight", "borderLeft"),
        ];
        if sides.iter().all(|(field, _)| view.number(field).is_some()) {
            for (field, prop) in sides {
                let weight = view.number(field).unwrap_or(0.0);
                if weight != 0.0 {
                    string_prop(props, prop, format!("{style} {} {color}", px(weight)));
                }
            }
        }
        return;
    }
    let weight = explicit_weight.unwrap_or(0.0);
    if view.node_type() == "LINE" {
        string_prop(props, "outline", format!("{style} {} {color}", px(weight)));
        let base = if view.string("layoutSizingHorizontal") == Some("FIXED") {
            view.number("width")
                .map(px)
                .unwrap_or_else(|| "100%".to_owned())
        } else {
            "100%".to_owned()
        };
        string_prop(
            props,
            "maxW",
            format!("calc({base} - {})", px(weight * 2.0)),
        );
        if view
            .number("rotation")
            .is_none_or(|rotation| rotation.abs() <= 0.01)
        {
            string_prop(
                props,
                "transform",
                format!("translate({}, {})", px(weight), px(-weight)),
            );
        }
        return;
    }
    if align == "INSIDE" {
        string_prop(props, "border", format!("{style} {} {color}", px(weight)));
    } else {
        string_prop(props, "outline", format!("{style} {} {color}", px(weight)));
        if align == "CENTER" {
            string_prop(props, "outlineOffset", px(-weight / 2.0));
        }
    }
}

fn push_effects(view: &TypedNode<'_>, component: &str, props: &mut Vec<Prop>) {
    let Some(effects) = view.value("effects").and_then(Value::as_array) else {
        return;
    };
    let visible = effects
        .iter()
        .filter(|effect| effect.get("visible").and_then(Value::as_bool) != Some(false))
        .collect::<Vec<_>>();
    let shadows = visible
        .iter()
        .filter(|effect| {
            matches!(
                effect.get("type").and_then(Value::as_str),
                Some("DROP_SHADOW" | "INNER_SHADOW")
            )
        })
        .filter_map(|effect| {
            let offset = effect.get("offset")?;
            let x = offset.get("x")?.as_f64()?;
            let y = offset.get("y")?.as_f64()?;
            let radius = effect.get("radius")?.as_f64()?;
            let spread = effect.get("spread").and_then(Value::as_f64).unwrap_or(0.0);
            let color = color_from(effect.get("color")?)?;
            let inset = if effect.get("type").and_then(Value::as_str) == Some("INNER_SHADOW") {
                "inset "
            } else {
                ""
            };
            Some(if component == "Text" {
                format!(
                    "{inset}{} {} {} {color}",
                    zero_or_px(x),
                    zero_or_px(y),
                    zero_or_px(radius)
                )
            } else {
                format!(
                    "{inset}{} {} {} {} {color}",
                    zero_or_px(x),
                    zero_or_px(y),
                    zero_or_px(radius),
                    zero_or_px(spread)
                )
            })
        })
        .collect::<Vec<_>>();
    if !shadows.is_empty() {
        string_prop(
            props,
            if component == "Text" {
                "textShadow"
            } else {
                "boxShadow"
            },
            shadows.join(", "),
        );
    }
    for effect in visible {
        let radius = effect.get("radius").and_then(Value::as_f64).unwrap_or(0.0);
        match effect.get("type").and_then(Value::as_str) {
            Some("LAYER_BLUR") => string_prop(props, "filter", format!("blur({})", px(radius))),
            Some("BACKGROUND_BLUR" | "GLASS") => {
                string_prop(props, "backdropFilter", format!("blur({})", px(radius)))
            }
            Some("NOISE" | "TEXTURE") => {
                string_prop(props, "filter", "contrast(100%) brightness(100%)")
            }
            _ => {}
        }
    }
}

/// Whether every visible effect on this node survives `push_effects` without
/// loss. Mirrors that function case for case; the two must move together.
///
/// `DEVUP_CODEGEN_EFFECT_FALLBACK` used to fire whenever a node merely *had* an
/// effects array. A plain drop shadow is present on nearly every real design,
/// so that permanently pinned `projection` to `lossy` and made `strict: true`
/// unusable, while saying nothing about what was actually lost.
///
/// Deliberately *not* counted as loss: `showShadowBehindNode`. CSS always
/// paints a non-inset `box-shadow` behind the element's box, so the flag only
/// changes rendering behind a translucent fill. Treating it as loss would put
/// essentially every Figma shadow back into `lossy` for a difference that is
/// usually invisible, recreating the problem this guard removes.
pub(super) fn effects_are_exact(view: &TypedNode<'_>) -> bool {
    let Some(effects) = view.value("effects").and_then(Value::as_array) else {
        return true;
    };
    // `push_effects` picks `textShadow` for Text, which has no spread slot.
    // `component.rs` resolves exactly this node type to the `Text` component.
    let is_text = view.node_type() == "TEXT";
    let visible = effects
        .iter()
        .filter(|effect| effect.get("visible").and_then(Value::as_bool) != Some(false))
        .collect::<Vec<_>>();

    // `push_effects` writes `filter` once per effect that maps to it, so two
    // such effects would collide on a single prop and the later one wins.
    let filter_writers = visible
        .iter()
        .filter(|effect| {
            matches!(
                effect.get("type").and_then(Value::as_str),
                Some("LAYER_BLUR" | "NOISE" | "TEXTURE")
            )
        })
        .count();
    if filter_writers > 1 {
        return false;
    }

    visible
        .iter()
        .all(|effect| match effect.get("type").and_then(Value::as_str) {
            Some("DROP_SHADOW" | "INNER_SHADOW") => {
                // Same fields `push_effects` requires before it emits a shadow;
                // if any is missing the effect is dropped on the floor.
                let renders = effect
                    .get("offset")
                    .and_then(|offset| {
                        Some((offset.get("x")?.as_f64()?, offset.get("y")?.as_f64()?))
                    })
                    .is_some()
                    && effect.get("radius").and_then(Value::as_f64).is_some()
                    && effect.get("color").and_then(color_from).is_some();
                // CSS shadows carry no per-shadow blend mode.
                let blend_survives = effect
                    .get("blendMode")
                    .and_then(Value::as_str)
                    .is_none_or(|mode| mode == "NORMAL");
                // `text-shadow` has no spread component.
                let spread_survives =
                    !is_text || effect.get("spread").and_then(Value::as_f64).unwrap_or(0.0) == 0.0;
                renders && blend_survives && spread_survives
            }
            // `push_effects` falls back to `blur(0px)` when the radius is
            // missing or unparseable, which silently fabricates the blur away.
            Some("LAYER_BLUR" | "BACKGROUND_BLUR") => {
                effect.get("radius").and_then(Value::as_f64).is_some()
            }
            // `GLASS` is flattened to a plain backdrop blur, `NOISE`/`TEXTURE`
            // become a no-op filter placeholder, and any other type is silently
            // ignored. All of those are real losses.
            _ => false,
        })
}

fn zero_or_px(value: f64) -> String {
    if value == 0.0 {
        "0".to_owned()
    } else {
        px(value)
    }
}

pub(super) fn first_solid_color(value: Option<&Value>) -> Option<String> {
    let paint = value?.as_array()?.iter().find(|paint| {
        paint.get("type").and_then(Value::as_str) == Some("SOLID")
            && paint.get("visible").and_then(Value::as_bool) != Some(false)
            && paint.get("opacity").and_then(Value::as_f64) != Some(0.0)
    })?;
    color_from_paint(paint)
}

fn color_from(color: &Value) -> Option<String> {
    let channel = |name: &str| -> Option<u8> {
        Some((color.get(name)?.as_f64()?.clamp(0.0, 1.0) * 255.0).round() as u8)
    };
    let alpha = color.get("a").and_then(Value::as_f64).unwrap_or(1.0);
    let mut hex = format!(
        "#{:02X}{:02X}{:02X}",
        channel("r")?,
        channel("g")?,
        channel("b")?
    );
    if alpha < 1.0 {
        hex.push_str(&format!(
            "{:02X}",
            (alpha.clamp(0.0, 1.0) * 255.0).round() as u8
        ));
    }
    let chars = hex.as_bytes();
    if (hex.len() == 7 || hex.len() == 9)
        && (1..hex.len())
            .step_by(2)
            .all(|index| chars[index] == chars[index + 1])
    {
        let mut short = String::from("#");
        for index in (1..hex.len()).step_by(2) {
            short.push(chars[index] as char);
        }
        Some(short)
    } else {
        Some(hex)
    }
}
