use std::collections::BTreeSet;

use devup_mcp_figma::{RawNode, Snapshot, TypedNode};
use serde_json::Value;

use super::{
    component::Prop,
    layout::{format_number, px, string_prop},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssetKind {
    Svg,
    SvgMask,
    Png,
}

pub(crate) fn asset_kind(snapshot: &Snapshot, node: &RawNode) -> Option<AssetKind> {
    asset_kind_nested(snapshot, node, false)
}

fn asset_kind_nested(snapshot: &Snapshot, node: &RawNode, nested: bool) -> Option<AssetKind> {
    let view = node.typed_view();
    if matches!(view.node_type(), "TEXT" | "COMPONENT_SET")
        || view
            .value("inferredAutoLayout")
            .and_then(|layout| layout.get("layoutMode"))
            .and_then(Value::as_str)
            == Some("GRID")
    {
        return None;
    }

    if has_smart_animate_reaction(node)
        || view
            .string("parentId")
            .and_then(|parent_id| snapshot.nodes.get(parent_id))
            .is_some_and(has_smart_animate_reaction)
    {
        return None;
    }

    if matches!(
        view.node_type(),
        "VECTOR" | "STAR" | "POLYGON" | "BOOLEAN_OPERATION"
    ) {
        return Some(svg_asset_kind(snapshot, node, nested));
    }

    if view.node_type() == "ELLIPSE"
        && view
            .value("arcData")
            .and_then(|arc_data| arc_data.get("innerRadius"))
            .and_then(Value::as_f64)
            .is_some_and(|inner_radius| inner_radius != 0.0)
    {
        return Some(svg_asset_kind(snapshot, node, nested));
    }

    let child_ids = view.child_ids().collect::<Vec<_>>();
    if child_ids.is_empty() {
        return leaf_asset_kind(snapshot, node, nested);
    }

    if child_ids.len() == 1 {
        if ["paddingLeft", "paddingRight", "paddingTop", "paddingBottom"]
            .into_iter()
            .any(|field| view.number(field).is_some_and(|padding| padding > 0.0))
            || fills(node).is_some_and(|fills| fills.iter().any(is_visible_fill))
        {
            return None;
        }

        return match snapshot
            .nodes
            .get(child_ids[0])
            .and_then(|child| asset_kind_nested(snapshot, child, true))
        {
            Some(AssetKind::Png) => Some(AssetKind::Png),
            Some(AssetKind::Svg | AssetKind::SvgMask) => {
                Some(svg_asset_kind(snapshot, node, nested))
            }
            None => None,
        };
    }

    let mut visible_children = Vec::new();
    for child_id in child_ids {
        let child = snapshot.nodes.get(child_id)?;
        if child.typed_view().bool("visible") != Some(false) {
            visible_children.push(child);
        }
    }

    visible_children
        .into_iter()
        .all(|child| {
            matches!(
                asset_kind_nested(snapshot, child, true),
                Some(AssetKind::Svg | AssetKind::SvgMask)
            )
        })
        .then(|| svg_asset_kind(snapshot, node, nested))
}

fn leaf_asset_kind(snapshot: &Snapshot, node: &RawNode, nested: bool) -> Option<AssetKind> {
    let node_fills = fills(node);
    if node_fills.is_some_and(|fills| {
        fills.iter().any(|fill| {
            is_visible_fill(fill)
                && (fill_type(fill) == Some("PATTERN")
                    || (fill_type(fill) == Some("IMAGE")
                        && fill.get("scaleMode").and_then(Value::as_str) == Some("TILE")))
        })
    }) {
        return None;
    }

    if node.typed_view().bool("isAsset") == Some(true) {
        if node_fills.is_some_and(|fills| {
            fills.iter().any(|fill| {
                is_visible_fill(fill)
                    && fill_type(fill) == Some("IMAGE")
                    && fill.get("scaleMode").and_then(Value::as_str) != Some("TILE")
            })
        }) {
            return (node_fills.is_some_and(|fills| fills.len() == 1)).then_some(AssetKind::Png);
        }

        if node_fills.is_none_or(|fills| {
            fills
                .iter()
                .all(|fill| is_visible_fill(fill) && fill_type(fill) == Some("SOLID"))
        }) {
            return nested.then(|| svg_asset_kind(snapshot, node, nested));
        }

        return Some(svg_asset_kind(snapshot, node, nested));
    }

    (nested
        && node_fills.is_some_and(|fills| {
            fills.iter().all(|fill| {
                !is_visible_fill(fill)
                    || !matches!(fill_type(fill), Some("IMAGE" | "VIDEO" | "PATTERN"))
            })
        }))
    .then(|| svg_asset_kind(snapshot, node, nested))
}

fn fills(node: &RawNode) -> Option<&Vec<Value>> {
    node.typed_view().value("fills").and_then(Value::as_array)
}

fn fill_type(fill: &Value) -> Option<&str> {
    fill.get("type").and_then(Value::as_str)
}

fn is_visible_fill(fill: &Value) -> bool {
    fill.get("visible").and_then(Value::as_bool) != Some(false)
}

fn has_smart_animate_reaction(node: &RawNode) -> bool {
    node.typed_view()
        .value("reactions")
        .and_then(Value::as_array)
        .is_some_and(|reactions| {
            reactions.iter().any(|reaction| {
                reaction
                    .get("actions")
                    .and_then(Value::as_array)
                    .is_some_and(|actions| {
                        actions.iter().any(|action| {
                            action.get("type").and_then(Value::as_str) == Some("NODE")
                                && action
                                    .get("transition")
                                    .and_then(|transition| transition.get("type"))
                                    .and_then(Value::as_str)
                                    == Some("SMART_ANIMATE")
                        })
                    })
            })
        })
}

fn svg_asset_kind(snapshot: &Snapshot, node: &RawNode, nested: bool) -> AssetKind {
    if matches!(
        same_color(snapshot, node, nested, None),
        SameColor::Color(_)
    ) {
        AssetKind::SvgMask
    } else {
        AssetKind::Svg
    }
}

/// What an asset is painted in, if it is one thing.
///
/// This is the `sameColor` half of the plugin's `computeAssetAnalysis`,
/// which decides whether an icon is drawn as an `<Image>` or as a Box masked
/// to its shape and filled with one colour. `Null` is a subtree that settled
/// on nothing, `False` one whose paints disagree, and only a `Color` makes a
/// mask. The two non-answers are not the same: a `Null` child leaves a
/// running colour alone, a `False` one spoils it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SameColor {
    Null,
    False,
    Color(String),
}

/// The plugin's `mergeSameColor`.
fn merge_same_color(current: SameColor, next: SameColor) -> SameColor {
    match (current, next) {
        (_, SameColor::False) => SameColor::False,
        (SameColor::Null, next) => next,
        (current, next) if current == next => current,
        _ => SameColor::False,
    }
}

/// The plugin's `analyzeOwnSameColor`: the node's own fills and strokes.
enum OwnColor {
    /// No visible paint at all.
    None,
    /// A paint that is not a flat colour.
    Null,
    /// Two flat colours that differ.
    False,
    Color(String),
}

fn own_same_color(
    node: &RawNode,
    variable_tokens: Option<&std::collections::BTreeMap<String, String>>,
) -> OwnColor {
    let view = node.typed_view();
    let mut target: Option<String> = None;
    let mut has_paints = false;
    for field in ["fills", "strokes"] {
        let Some(paints) = view.value(field).and_then(Value::as_array) else {
            continue;
        };
        for paint in paints {
            if paint.get("visible").and_then(Value::as_bool) != Some(true) {
                continue;
            }
            has_paints = true;
            if paint.get("type").and_then(Value::as_str) != Some("SOLID") {
                return OwnColor::Null;
            }
            let Some(color) = paint_string(paint, variable_tokens) else {
                return OwnColor::Null;
            };
            match &target {
                None => target = Some(color),
                Some(current) if *current != color => return OwnColor::False,
                Some(_) => {}
            }
        }
    }
    if !has_paints {
        return OwnColor::None;
    }
    match target {
        Some(color) => OwnColor::Color(color),
        None => OwnColor::Null,
    }
}

/// A solid paint as the plugin's `solidToString` spells it: the variable it
/// is bound to as `$token`, or else the colour. Two paints are the same
/// colour to the plugin only when these agree, so a paint bound to a
/// variable and a raw paint of the same hex are *not* the same.
///
/// Without a token map the variable id stands in for its name; that keeps
/// the comparison right when only the shape is being decided and the names
/// are not to hand.
pub(super) fn paint_string(
    paint: &Value,
    variable_tokens: Option<&std::collections::BTreeMap<String, String>>,
) -> Option<String> {
    if let Some(id) = paint
        .get("boundVariables")
        .and_then(|bound| bound.get("color"))
        .and_then(|color| color.get("id"))
        .and_then(Value::as_str)
    {
        match variable_tokens {
            None => return Some(format!("${id}")),
            Some(tokens) => {
                if let Some(token) = tokens.get(id) {
                    return Some(format!("${token}"));
                }
            }
        }
    }
    if paint.get("opacity").and_then(Value::as_f64) == Some(0.0) {
        return Some("transparent".to_owned());
    }
    color_from_paint(paint)
}

fn same_color(
    snapshot: &Snapshot,
    node: &RawNode,
    nested: bool,
    variable_tokens: Option<&std::collections::BTreeMap<String, String>>,
) -> SameColor {
    let view = node.typed_view();
    let own = || match own_same_color(node, variable_tokens) {
        OwnColor::Color(color) => SameColor::Color(color),
        _ => SameColor::Null,
    };
    if matches!(view.node_type(), "TEXT" | "COMPONENT_SET")
        || view
            .value("inferredAutoLayout")
            .and_then(|layout| layout.get("layoutMode"))
            .and_then(Value::as_str)
            == Some("GRID")
        || has_smart_animate_reaction(node)
        || view
            .string("parentId")
            .and_then(|parent_id| snapshot.nodes.get(parent_id))
            .is_some_and(has_smart_animate_reaction)
    {
        return SameColor::Null;
    }
    // Boolean operands define geometry; the result's own paint colors it.
    if matches!(
        view.node_type(),
        "VECTOR" | "STAR" | "POLYGON" | "BOOLEAN_OPERATION"
    ) {
        return own();
    }
    if view.node_type() == "ELLIPSE"
        && view
            .value("arcData")
            .and_then(|arc_data| arc_data.get("innerRadius"))
            .and_then(Value::as_f64)
            .is_some_and(|inner_radius| inner_radius != 0.0)
    {
        return own();
    }

    let child_ids = view.child_ids().collect::<Vec<_>>();
    if child_ids.is_empty() {
        let node_fills = fills(node);
        if node_fills.is_some_and(|fills| {
            fills.iter().any(|fill| {
                is_visible_fill(fill)
                    && (fill_type(fill) == Some("PATTERN")
                        || (fill_type(fill) == Some("IMAGE")
                            && fill.get("scaleMode").and_then(Value::as_str) == Some("TILE")))
            })
        }) {
            return SameColor::Null;
        }
        if view.bool("isAsset") == Some(true) {
            let Some(node_fills) = node_fills else {
                return SameColor::Null;
            };
            if node_fills.iter().any(|fill| {
                is_visible_fill(fill)
                    && fill_type(fill) == Some("IMAGE")
                    && fill.get("scaleMode").and_then(Value::as_str) != Some("TILE")
            }) {
                return SameColor::Null;
            }
            if node_fills.iter().all(|fill| {
                fill.get("visible").and_then(Value::as_bool) == Some(true)
                    && fill_type(fill) == Some("SOLID")
            }) {
                return if nested { own() } else { SameColor::Null };
            }
            return match own_same_color(node, variable_tokens) {
                OwnColor::Color(color) => SameColor::Color(color),
                OwnColor::False => SameColor::False,
                _ => SameColor::Null,
            };
        }
        if nested
            && node_fills.is_some_and(|fills| {
                !fills.iter().any(|fill| {
                    is_visible_fill(fill)
                        && matches!(fill_type(fill), Some("IMAGE" | "VIDEO" | "PATTERN"))
                })
            })
        {
            return own();
        }
        return SameColor::Null;
    }

    if child_ids.len() == 1 {
        if ["paddingLeft", "paddingRight", "paddingTop", "paddingBottom"]
            .into_iter()
            .any(|field| view.number(field).is_some_and(|padding| padding > 0.0))
            || fills(node).is_some_and(|fills| fills.iter().any(is_visible_fill))
        {
            return SameColor::Null;
        }
        return snapshot
            .nodes
            .get(child_ids[0])
            .map_or(SameColor::Null, |child| {
                same_color(snapshot, child, true, variable_tokens)
            });
    }

    let mut same = match own_same_color(node, variable_tokens) {
        OwnColor::Null => return SameColor::Null,
        OwnColor::False => return SameColor::False,
        OwnColor::Color(color) => SameColor::Color(color),
        OwnColor::None => SameColor::Null,
    };
    for child in child_ids
        .into_iter()
        .filter_map(|id| snapshot.nodes.get(id))
        .filter(|child| child.typed_view().bool("visible") != Some(false))
    {
        same = merge_same_color(same, same_color(snapshot, child, true, variable_tokens));
    }
    same
}

/// What the style pass needs beyond the node itself: which variables carry a
/// token name, and how assets are to be named. Carried together so that
/// adding to it does not lengthen every signature it passes through.
#[derive(Clone, Copy)]
pub(super) struct StyleOptions<'a> {
    pub variable_tokens: &'a std::collections::BTreeMap<String, String>,
    pub asset_names_per_node: bool,
}

pub(super) fn push_style_props(
    snapshot: &Snapshot,
    node: &RawNode,
    component: &str,
    asset: Option<AssetKind>,
    props: &mut Vec<Prop>,
    used_tokens: &mut BTreeSet<String>,
    style: StyleOptions<'_>,
) {
    let StyleOptions {
        variable_tokens,
        asset_names_per_node: per_node,
    } = style;
    let view = node.typed_view();
    if view.bool("visible") == Some(false) {
        string_prop(props, "display", "none");
    }
    if non_rendering_asset_reason(snapshot, node).is_some() {
        // Background paints and the variant tree also pass here. Do not
        // leave a URL behind merely because the node is not an Image leaf.
        string_prop(props, "visibility", "hidden");
        return;
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
        let source = asset_source(snapshot, node, folder, extension, per_node);
        if asset == AssetKind::SvgMask {
            if let SameColor::Color(color) =
                same_color(snapshot, node, false, Some(variable_tokens))
            {
                if let Some(token) = color.strip_prefix('$') {
                    used_tokens.insert(token.to_owned());
                }
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
        push_object_fit(&view, props);
        // An export in flow is drawn where it sits in the box the layout gives
        // the node. They coincide for a plain icon; the notice logo is an
        // instance of 1373x98 whose vector is 952x104 at 425px in, and
        // `contain` centred a 1373-wide picture of a 952-wide logo. The
        // element keeps the layout's box, and the picture is placed inside it
        // at the export's own size and offset. An element the layout
        // positions is placed by its export outright, in `codegen::layout`.
        if view.string("layoutPositioning") != Some("ABSOLUTE")
            && !super::layout::placed_by_a_free_layout(
                snapshot,
                node,
                view.string("parentId")
                    .and_then(|parent_id| snapshot.nodes.get(parent_id)),
                false,
            )
            && let Some(offset) = super::layout::export_offset(node)
        {
            let size = format!("{} {}", px(offset.w), px(offset.h));
            let position = format!("{} {}", px(offset.x), px(offset.y));
            if asset == AssetKind::SvgMask {
                string_prop(props, "maskSize", size);
                string_prop(props, "maskPos", position);
            } else {
                string_prop(props, "objectFit", "none");
                string_prop(props, "objectPos", position);
            }
        }
        push_radius(&view, props);
        push_strokes(&view, props, used_tokens, variable_tokens);
        push_effects(&view, component, props, used_tokens, variable_tokens);
        // An export carries the node's own opacity: Figma writes it into the
        // SVG as `<g opacity>` and into a PNG's alpha. Written on the element
        // as well it is applied twice - a decoration at 0.2 came out at 0.04,
        // which is nothing, and the landing page's hero at 0.8 came out at
        // 0.64. The plugin writes it twice too. A mask is the same: the
        // SVG's own opacity thins the mask, so the colour painted through it
        // already shows at the node's opacity.
        push_blend_mode(&view, props);
        return;
    }

    push_object_fit(&view, props);
    let color_prop = if component == "Text" { "color" } else { "bg" };
    // A background is every visible paint, back to front, as the plugin's
    // `getBackgroundProps` composes it. Reading only the bound variable
    // dropped the photo that sits on top of a `$gray200` plate: the about
    // member cards are `url(...) center/cover no-repeat, $gray200`, and were
    // coming out as the plate alone.
    let layered = component != "Text"
        && view
            .value("fills")
            .and_then(Value::as_array)
            .is_some_and(|fills| {
                fills
                    .iter()
                    .filter(|paint| {
                        paint.get("visible").and_then(Value::as_bool) != Some(false)
                            && paint.get("opacity").and_then(Value::as_f64) != Some(0.0)
                    })
                    .count()
                    > 1
            });
    if layered {
        if let Some(background) = background_css(snapshot, node, used_tokens, style) {
            string_prop(props, "bg", background);
        }
    } else if let Some(token) = view
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
        if let Some(background) = background_css(snapshot, node, used_tokens, style) {
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
    } else if let Some(background) = background_css(snapshot, node, used_tokens, style) {
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
    push_effects(&view, component, props, used_tokens, variable_tokens);
    if let Some(opacity) = view.number("opacity")
        && opacity < 1.0
    {
        string_prop(props, "opacity", format_number(opacity));
    }
    push_blend_mode(&view, props);
}

pub(super) fn non_rendering_asset_reason(
    snapshot: &Snapshot,
    node: &RawNode,
) -> Option<&'static str> {
    let reason = if node
        .typed_view()
        .value("absoluteRenderBounds")
        .is_some_and(Value::is_null)
    {
        "no-render-bounds"
    } else {
        devup_mcp_figma::asset_exclusion_reason(node)?
    };
    (asset_kind(snapshot, node).is_some()
        || fills(node)
            .is_some_and(|paints| paints.iter().any(|paint| fill_type(paint) == Some("IMAGE"))))
    .then_some(reason)
}

/// The plugin's `getObjectFitProps`: how the first visible image fill of a
/// node Figma calls an asset is scaled. It is written whatever element the
/// node became — the about member cards are a `Box` whose photo sits on a
/// `$gray200` plate, and the reference gives them `objectFit="cover"` all the
/// same. `FILL` and `TILE` say nothing.
fn push_object_fit(view: &TypedNode<'_>, props: &mut Vec<Prop>) {
    if view.bool("isAsset") != Some(true) {
        return;
    }
    let Some(scale) = view
        .value("fills")
        .and_then(Value::as_array)
        .and_then(|fills| {
            fills.iter().find(|paint| {
                paint.get("type").and_then(Value::as_str) == Some("IMAGE")
                    && paint.get("visible").and_then(Value::as_bool) == Some(true)
            })
        })
        .and_then(|paint| paint.get("scaleMode"))
        .and_then(Value::as_str)
    else {
        return;
    };
    match scale {
        "FIT" => string_prop(props, "objectFit", "contain"),
        "CROP" => string_prop(props, "objectFit", "cover"),
        _ => {}
    }
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
    used_tokens: &mut BTreeSet<String>,
    style: StyleOptions<'_>,
) -> Option<String> {
    let view = node.typed_view();
    let paints = view.value("fills")?.as_array()?;
    // Keep each paint's own index. CSS layers run back to front, so the order
    // here is reversed, but an image fill is identified in the asset manifest
    // as `{nodeId}:fills:{index}` against the original order — a reference
    // built from the reversed position would name the wrong asset.
    let visible = paints
        .iter()
        .enumerate()
        .filter(|(_, paint)| {
            paint.get("visible").and_then(Value::as_bool) != Some(false)
                && paint.get("opacity").and_then(Value::as_f64) != Some(0.0)
        })
        .rev()
        .collect::<Vec<_>>();
    let mut css = Vec::new();
    for (layer, (fill_index, paint)) in visible.iter().enumerate() {
        let is_last = layer + 1 == visible.len();
        if let Some(value) = paint_css(
            snapshot,
            node,
            paint,
            *fill_index,
            is_last,
            used_tokens,
            style,
        ) {
            css.push(value);
        }
    }
    (!css.is_empty()).then(|| css.join(", "))
}

/// The file an image fill refers to.
///
/// Every image fill once resolved to a single hard-coded `/icons/image.png`,
/// which lost three separate things: a raster was pointed at the icon folder,
/// unrelated images from different nodes all claimed the same file and so
/// overwrote one another on disk, and two fills on one node produced the
/// identical URL twice over. The manifest identifies a fill as
/// `{nodeId}:fills:{index}`, so the reference keeps the node's name and, past
/// the first fill, its index — a lone fill keeps the plain
/// `/images/{name}.png` the `<Image>` element already emits, so the two agree
/// on the same asset.
fn image_fill_source(
    snapshot: &Snapshot,
    node: &RawNode,
    fill_index: usize,
    per_node: bool,
) -> String {
    let source = if fill_index == 0 {
        asset_source(snapshot, node, "images", "png", per_node)
    } else {
        let stem = asset_stem(snapshot, node, per_node);
        format!("/images/{stem}-{fill_index}.png")
    };
    if source.contains(' ') {
        format!("'{source}'")
    } else {
        source
    }
}

/// The file an asset node is drawn from: `/{folder}/{stem}.{extension}`.
///
/// The plugin draws an instance from its main component and names the file
/// after that node, so every instance of a variant is `Property 1=search.svg`:
/// one file for the icon wherever it is used, but the same file for every
/// component set that has a `search` variant, each overwriting the last. The
/// layer name a designer gave the node is used instead, and where two assets
/// in the snapshot that are not the same thing would share it, each says what
/// it is: see `asset_stem`.
pub(crate) fn asset_source(
    snapshot: &Snapshot,
    node: &RawNode,
    folder: &str,
    extension: &str,
    per_node: bool,
) -> String {
    format!(
        "/{folder}/{}.{extension}",
        asset_stem(snapshot, node, per_node)
    )
}

/// The path the generated code refers to for an asset node - `/icons/x.svg`
/// for a vector, `/images/x.png` for the first image fill - so a manifest
/// can say where the code expects each asset. `None` for a node the code
/// does not draw from a file.
pub fn asset_path(snapshot: &Snapshot, node_id: &str, per_node: bool) -> Option<String> {
    let node = snapshot.nodes.get(node_id)?;
    let kind = asset_kind(snapshot, node)?;
    let (folder, extension) = match kind {
        AssetKind::Svg | AssetKind::SvgMask => ("icons", "svg"),
        _ => ("images", "png"),
    };
    Some(asset_source(snapshot, node, folder, extension, per_node))
}

/// The `position/size` a cropped image fill is painted with, read from
/// Figma's `imageTransform`. The matrix maps the image's own 0..1 space onto
/// the box: the part on show runs from `tx` for `sx` across and from `ty` for
/// `sy` down. Scaling the picture by `1/sx` makes that part as wide as the
/// box, and `tx / (1 - sx)` is where along the overflow it has to sit - which
/// is exactly the percentage CSS positions a background by. A scale of one
/// leaves no overflow to position within, so it sits at the start.
fn image_crop(paint: &Value) -> Option<String> {
    let rows = paint.get("imageTransform")?.as_array()?;
    let cell = |row: usize, column: usize| rows.get(row)?.as_array()?.get(column)?.as_f64();
    let (scale_x, offset_x) = (cell(0, 0)?, cell(0, 2)?);
    let (scale_y, offset_y) = (cell(1, 1)?, cell(1, 2)?);
    if scale_x == 0.0 || scale_y == 0.0 {
        return None;
    }
    let position = |scale: f64, offset: f64| {
        if (1.0 - scale).abs() < 1e-6 {
            0.0
        } else {
            offset / (1.0 - scale) * 100.0
        }
    };
    Some(format!(
        "{}% {}%/{}% {}%",
        format_number(position(scale_x, offset_x)),
        format_number(position(scale_y, offset_y)),
        format_number(100.0 / scale_x),
        format_number(100.0 / scale_y),
    ))
}

/// Where the code draws one of a node's image fills from: `/images/x.png`
/// for the first fill and `/images/x-2.png` past it, the same name
/// `image_fill_source` writes into the code but without the quoting a CSS
/// `url()` puts around a name with a space in it.
///
/// This answers for any node that carries the fill, where `asset_path` only
/// answers for a node the code draws entirely from a file. A section painted
/// over a photograph is a layout box holding children, not an asset - but the
/// photograph on it is still a file the code points at, and a caller has to
/// be told where.
pub fn image_fill_path(
    snapshot: &Snapshot,
    node_id: &str,
    fill_index: usize,
    per_node: bool,
) -> Option<String> {
    let node = snapshot.nodes.get(node_id)?;
    let stem = asset_stem(snapshot, node, per_node);
    Some(if fill_index == 0 {
        format!("/images/{stem}.png")
    } else {
        format!("/images/{stem}-{fill_index}.png")
    })
}

/// The file name an asset node gets, without folder or extension.
///
/// The layer name, unless another asset in the snapshot has the same name
/// and is a different thing. Three cards each hold an `Icons` instance at a
/// different variant - chart, clock, lightning - and all three were
/// `/icons/Icons.svg`, one file overwriting the next, and every card drew the
/// chart. An instance whose name is shared then carries its variant, `Icons=chart`;
/// a node that is not an instance carries its id. Instances of one variant
/// share a name and a file, as the same icon at three widths should.
pub(crate) fn asset_stem(snapshot: &Snapshot, node: &RawNode, per_node: bool) -> String {
    let view = node.typed_view();
    let name = view.name().unwrap_or("Asset");
    let identity = asset_identity(node);
    let shared = per_node
        || snapshot.nodes.values().any(|other| {
            other.id != node.id
                && other.typed_view().name() == Some(name)
                && asset_identity(other) != identity
                && asset_kind(snapshot, other).is_some()
        });
    if !shared {
        return name.to_owned();
    }
    match identity {
        Some(variant) => format!("{name}={variant}"),
        None => format!("{name}-{}", node.id.replace([':', ';'], "-")),
    }
}

/// What makes an instance the thing it is: its variant, as `chart` or
/// `lg,primary`. `None` for a node that is not an instance of a variant.
fn asset_identity(node: &RawNode) -> Option<String> {
    let view = node.typed_view();
    let properties = view.value("variantProperties")?.as_object()?;
    let values = properties
        .values()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    (!values.is_empty()).then(|| values.join(","))
}

fn paint_css(
    snapshot: &Snapshot,
    node: &RawNode,
    paint: &Value,
    fill_index: usize,
    last: bool,
    used_tokens: &mut BTreeSet<String>,
    style: StyleOptions<'_>,
) -> Option<String> {
    let StyleOptions {
        variable_tokens,
        asset_names_per_node: per_node,
    } = style;
    let kind = paint.get("type")?.as_str()?;
    match kind {
        "SOLID" => {
            let color = bound_paint_token(paint, variable_tokens)
                .map(|token| {
                    used_tokens.insert(token.clone());
                    format!("${token}")
                })
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
            let source = image_fill_source(snapshot, node, fill_index, per_node);
            // A cropped fill carries its crop as a matrix over the image's own
            // 0..1 space. Painted `center/cover` that is thrown away and the
            // whole picture is shown instead, which is a different crop: the
            // about page's photographs came out zoomed in against the render
            // Figma draws of the same frame.
            if paint.get("scaleMode").and_then(Value::as_str) == Some("CROP")
                && let Some(crop) = image_crop(paint)
            {
                return Some(format!("url({source}) {crop} no-repeat"));
            }
            let fit = match paint.get("scaleMode").and_then(Value::as_str) {
                Some("FIT") => "center/contain no-repeat",
                Some("FILL" | "CROP") => "center/cover no-repeat",
                Some("TILE") => "repeat",
                _ => "center/cover no-repeat",
            };
            Some(format!("url({source}) {fit}"))
        }
        "PATTERN" => {
            let source_id = paint.get("sourceNodeId").and_then(Value::as_str)?;
            let source = snapshot.nodes.get(source_id);
            let name = source
                .and_then(|node| node.typed_view().name())
                .unwrap_or("pattern");
            // A raster belongs with the images and a vector with the icons,
            // which is the split every other asset reference follows. This one
            // sent a png to the icon folder.
            let raster = source
                .and_then(|node| asset_kind(snapshot, node))
                .is_some_and(|kind| kind == AssetKind::Png);
            let (folder, extension) = if raster {
                ("images", "png")
            } else {
                ("icons", "svg")
            };
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
                "url(/{folder}/{name}.{extension}){} repeat",
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
            // A stop bound to a variable is the token — and where the stop or
            // the paint is translucent, the token mixed with transparent by
            // that much, as the plugin's `processGradientStopColor` writes it:
            // a token names an opaque colour, and the alpha would be lost with
            // it. The report section's backdrop is a 50% gradient between two
            // tokens, `color-mix(in srgb, $primaryBg, transparent 50%)`.
            let color = bound_paint_token(stop, variable_tokens)
                .map(|token| {
                    if alpha < 1.0 {
                        format!(
                            "color-mix(in srgb, ${token}, transparent {}%)",
                            format_number((1.0 - alpha) * 100.0)
                        )
                    } else {
                        format!("${token}")
                    }
                })
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
        // A diamond is four linear gradients, one into each corner. The pairs
        // are written as pairs rather than as `"corner|direction"` strings
        // split back apart at runtime. Nothing outside this array ever reached
        // that split — the design comes in through `raw_stops`, `transform`
        // and `kind`, all of which are already `Option`-handled above — so the
        // only way it could have failed was a typo in the four lines below,
        // and it would have failed by killing the server rather than by
        // drawing a wrong gradient. Said as tuples, the same four facts cannot
        // be written malformed at all.
        "diamond" => [
            ("bottom right", "to bottom right"),
            ("bottom left", "to bottom left"),
            ("top left", "to top left"),
            ("top right", "to top right"),
        ]
        .map(|(position, direction)| {
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

fn push_effects(
    view: &TypedNode<'_>,
    component: &str,
    props: &mut Vec<Prop>,
    used_tokens: &mut BTreeSet<String>,
    variable_tokens: &std::collections::BTreeMap<String, String>,
) {
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
            // A shadow's colour can be bound to a variable, exactly as a fill
            // or a stroke can, and then the token is what the design means:
            // the landing page's cards are `$shadow`, one value the theme can
            // move for dark mode. Written as the resolved `#87878740` they
            // were a colour nothing could reach.
            let color = if let Some(token) = bound_paint_token(effect, variable_tokens) {
                used_tokens.insert(token.clone());
                format!("${token}")
            } else {
                color_from(effect.get("color")?)?
            };
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
