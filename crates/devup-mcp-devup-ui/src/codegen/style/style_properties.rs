use std::collections::{BTreeMap, BTreeSet};
use devup_mcp_figma::{RawNode, Snapshot, TypedNode};
use serde_json::Value;
use super::{
    component::{LengthToken, Prop, PropValue},
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
        if child.typed_view().bool("visible") != Some(false)
            || child.typed_view().value("componentPropertyReferences")
                .and_then(|references| references.get("visible"))
                .and_then(Value::as_str)
                .is_some()
        {
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
///
/// A paint that is not one flat colour (a gradient, an image, a solid with
/// no readable colour) is `False`, not `Null`. A mask keeps only the shape
/// and fills it with one colour, so letting such a paint stay neutral merged
/// a gradient face and a solid outline into one solid blob.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SameColor {
    Null,
    False,
    Color(String),
}

/// The plugin's `mergeSameColor`, except that a `Null` (a subtree with no
/// paint) is neutral wherever it falls. Taken literally, a `Null` after the
/// first colour spoiled it while one before was skipped, so the order of the
/// layers decided whether an icon became a mask.
fn merge_same_color(current: SameColor, next: SameColor) -> SameColor {
    match (current, next) {
        (_, SameColor::False) => SameColor::False,
        (SameColor::Null, next) => next,
        (current, SameColor::Null) => current,
        (current, next) if current == next => current,
        _ => SameColor::False,
    }
}

/// The plugin's `analyzeOwnSameColor`: the node's own fills and strokes.
enum OwnColor {
    /// No visible paint at all.
    None,
    /// Two flat colours that differ, or a paint that is not a flat colour.
    False,
    Color(String),
}

impl OwnColor {
    fn same_color(self) -> SameColor {
        match self {
            Self::None => SameColor::Null,
            Self::False => SameColor::False,
            Self::Color(color) => SameColor::Color(color),
        }
    }
}

fn own_same_color(
    node: &RawNode,
    variable_tokens: Option<&std::collections::BTreeMap<String, String>>,
) -> OwnColor {
    let view = node.typed_view();
    let mut target: Option<String> = None;
    for field in ["fills", "strokes"] {
        let Some(paints) = view.value(field).and_then(Value::as_array) else {
            continue;
        };
        for paint in paints {
            if paint.get("visible").and_then(Value::as_bool) != Some(true) {
                continue;
            }
            if paint.get("type").and_then(Value::as_str) != Some("SOLID") {
                return OwnColor::False;
            }
            let Some(color) = paint_string(paint, variable_tokens) else {
                return OwnColor::False;
            };
            match &target {
                None => target = Some(color),
                Some(current) if *current != color => return OwnColor::False,
                Some(_) => {}
            }
        }
    }
    target.map_or(OwnColor::None, OwnColor::Color)
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
    let own = || own_same_color(node, variable_tokens).same_color();
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
                // A picture inside an icon has colours a mask cannot keep.
                return SameColor::False;
            }
            if node_fills.iter().all(|fill| {
                fill.get("visible").and_then(Value::as_bool) == Some(true)
                    && fill_type(fill) == Some("SOLID")
            }) {
                return if nested { own() } else { SameColor::Null };
            }
            return own();
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
        OwnColor::False => return SameColor::False,
        own => own.same_color(),
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
    pub reserve_image_content: bool,
    pub asset_policies: &'a BTreeMap<String, super::AssetRepresentationPolicy>,
    pub variable_tokens: &'a std::collections::BTreeMap<String, String>,
    pub length_tokens: &'a std::collections::BTreeMap<String, LengthToken>,
    pub asset_names_per_node: bool,
    pub width_policy: Option<&'a super::width::WidthPolicy>,
}

// Record the route at the property writer; available geometry alone is not
// evidence that a boundary override actually ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StyleDerivation {
    SvgMaskPolicy,
    InFlowExportBoundary,
    ImagePaintScale,
    NativeRaster,
    WidthPolicyLine,
}

impl StyleDerivation {
    pub(super) fn path(self) -> &'static str {
        match self {
            Self::SvgMaskPolicy => "svg-mask-policy",
            Self::InFlowExportBoundary => "in-flow-export-boundary",
            Self::ImagePaintScale => "image-paint-scale",
            Self::NativeRaster => "native-raster-boundary",
            Self::WidthPolicyLine => "width-policy-straight-line",
        }
    }

    pub(super) fn description(self) -> (Vec<&'static str>, &'static str) {
        match self {
            Self::SvgMaskPolicy => (
                vec![
                    "fills",
                    "childrenIds",
                    "isAsset",
                    "absoluteRenderBounds",
                    "absoluteBoundingBox",
                ],
                "SVG mask projection policy from asset_kind/same_color: maskRepeat=no-repeat, maskSize=contain, maskPos=center unless export boundary derivation overrides them.",
            ),
            Self::InFlowExportBoundary => (
                vec![
                    "absoluteRenderBounds",
                    "absoluteBoundingBox",
                    "layoutPositioning",
                    "parentId",
                    "fills",
                ],
                include_str!("../../text/style-description-5ad1a929.md"),
            ),
            Self::NativeRaster => (
                vec!["isAsset", "fills", "absoluteRenderBounds"],
                include_str!("../../text/style-description-49ef22f0.md"),
            ),
            Self::ImagePaintScale => (
                vec!["isAsset", "fills"],
                "push_object_fit: first visible IMAGE paint scaleMode FIT -> contain; CROP -> cover.",
            ),
            Self::WidthPolicyLine => (
                vec![
                    "absoluteBoundingBox",
                    "strokes",
                    "strokeWeight",
                    "rotation",
                    "constraints",
                ],
                include_str!("../../text/style-description-b7e8995c.md"),
            ),
        }
    }
}

#[derive(Default)]
pub(super) struct StyleDerivations(BTreeMap<String, (String, StyleDerivation)>);

impl StyleDerivations {
    fn push(
        &mut self,
        props: &mut Vec<Prop>,
        name: &str,
        value: impl Into<String>,
        route: StyleDerivation,
    ) {
        let value = value.into();
        string_prop(props, name, value.clone());
        // Last writer wins for both the value and its explanation.
        self.0.insert(name.to_owned(), (value, route));
    }

    pub(super) fn matching(&self, name: &str, value: &str) -> Option<StyleDerivation> {
        let (written, route) = self.0.get(name)?;
        (written == value).then_some(*route)
    }
}

pub(super) fn push_style_props(
    snapshot: &Snapshot,
    node: &RawNode,
    component: &str,
    asset: Option<AssetKind>,
    props: &mut Vec<Prop>,
    used_tokens: &mut BTreeSet<String>,
    style: StyleOptions<'_>,
) -> StyleDerivations {
    let mut derivations = StyleDerivations::default();
    let StyleOptions {
        variable_tokens,
        length_tokens,
        asset_names_per_node: per_node,
        width_policy,
        asset_policies,
        reserve_image_content,
    } = style;
    let view = node.typed_view();
    let lengths = Lengths {
        tokens: length_tokens,
        viewport: drawn_viewport(snapshot, node),
    };
    // Reset native faces even with transparent paint, including Figma button
    // components that consumers render polymorphically.
    let explicit_button = view.string("as") == Some("button")
        || view.node_type().eq_ignore_ascii_case("button");
    let button_name = |name: &str| {
        name.split(|c: char| !c.is_alphanumeric())
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case("button"))
    };
    let named_button = component != "Text"
        && (view.name().is_some_and(button_name)
            || snapshot.nodes.values().any(|parent| {
                let parent = parent.typed_view();
                parent.node_type() == "COMPONENT_SET"
                    && parent.name().is_some_and(button_name)
                    && parent.child_ids().any(|id| id == node.id)
            }));
    if explicit_button || named_button || component.eq_ignore_ascii_case("button") {
        if explicit_button {
            string_prop(props, "as", "button");
        }
        string_prop(props, "appearance", "none");
    }
    if view.bool("visible") == Some(false) {
        string_prop(props, "display", "none");
    }
    if non_rendering_asset_reason(snapshot, node).is_some() {
        // Background paints and the variant tree also pass here. Do not
        // leave a URL behind merely because the node is not an Image leaf.
        string_prop(props, "visibility", "hidden");
        return derivations;
    }
    if let Some(asset) = asset {
        let image_index = super::image::selected_image_index(node, asset_policies);
        let image_id = image_index
            .map(|i| format!("{}:fills:{i}", node.id))
            .unwrap_or_else(|| node.id.clone());
        let policy = super::image::policy_for(node, &image_id, asset_policies);
        let original = asset == AssetKind::Png && image_index.is_some() && policy.original();
        if policy.incompatible_capture(&node.id) {
            string_prop(props, "visibility", "hidden");
            return derivations;
        }
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
        let source = (asset == AssetKind::Png)
            .then(|| policy.asset_path.clone())
            .flatten()
            .unwrap_or_else(|| asset_source(snapshot, node, folder, extension, per_node));
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
            derivations.push(
                props,
                "maskRepeat",
                "no-repeat",
                StyleDerivation::SvgMaskPolicy,
            );
            derivations.push(props, "maskSize", "contain", StyleDerivation::SvgMaskPolicy);
            derivations.push(props, "maskPos", "center", StyleDerivation::SvgMaskPolicy);
            if width_policy
                .is_some_and(|policy| super::width::line_follows_width(snapshot, node, policy))
            {
                derivations.push(
                    props,
                    "maskSize",
                    "100% 100%",
                    StyleDerivation::WidthPolicyLine,
                );
            }
        } else {
            string_prop(props, "src", source);
        }
        if original {
            let mapping =
                super::image::map_for(snapshot, node, image_index, asset_policies, per_node);
            if let Some(path) = mapping["assetPath"].as_str() {
                string_prop(props, "src", path);
            }
            super::image::apply_image_props(props, &mapping);
            if reserve_image_content {
                super::image::reserve_hug_content(props, &mapping);
            }
            if policy.effects_baked != Some(true) {
                let node_opacity = view.number("opacity").unwrap_or(1.);
                let paint_opacity = if mapping["css"]["image"]["transform"].is_null() {
                    mapping["css"]["image"]["opacity"]
                        .as_str()
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(1.)
                } else {
                    1.
                };
                if node_opacity * paint_opacity < 1. {
                    string_prop(
                        props,
                        "opacity",
                        format_number(node_opacity * paint_opacity),
                    );
                }
            }
        } else if asset != AssetKind::Png {
            push_object_fit(&view, props, &mut derivations);
        } else {
            derivations.push(props, "objectFit", "fill", StyleDerivation::NativeRaster);
            let mapping =
                super::image::map_for(snapshot, node, image_index, asset_policies, per_node);
            super::image::apply_image_props(props, &mapping);
        }
        // An export in flow is drawn where it sits in the box the layout gives
        // the node. They coincide for a plain icon; the notice logo is an
        // instance of 1373x98 whose vector is 952x104 at 425px in, and
        // `contain` centred a 1373-wide picture of a 952-wide logo. The
        // element keeps the layout's box, and the picture is placed inside it
        // at the export's own size and offset. An element the layout
        // positions is placed by its export outright, in `codegen::layout`.
        if !original
            && view.string("layoutPositioning") != Some("ABSOLUTE")
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
                derivations.push(
                    props,
                    "maskSize",
                    size,
                    StyleDerivation::InFlowExportBoundary,
                );
                derivations.push(
                    props,
                    "maskPos",
                    position,
                    StyleDerivation::InFlowExportBoundary,
                );
            } else {
                derivations.push(
                    props,
                    "objectFit",
                    "none",
                    StyleDerivation::InFlowExportBoundary,
                );
                derivations.push(
                    props,
                    "objectPos",
                    position,
                    StyleDerivation::InFlowExportBoundary,
                );
            }
        }
        if asset != AssetKind::Png
            || (policy.effects_baked != Some(true)
                && (policy.effects_baked == Some(false) || original))
        {
            push_radius(&view, props);
            // The exported asset has its own painted bounds. The live-container
            // outside-edge correction does not establish how to compose those.
            push_strokes(&view, None, props, used_tokens, variable_tokens);
            // Native PNGs flatten this entire subtree, including its own blur and
            // shadows. Layout ancestors still render their effects in their own
            // branch below. SVG masks are alpha masks rather than raster pictures;
            // retain their existing compositing policy.
            if asset != AssetKind::Png || original || policy.effects_baked == Some(false) {
                push_effects(
                    &view,
                    component,
                    props,
                    used_tokens,
                    variable_tokens,
                    lengths,
                );
            }
            if !original
                && policy.effects_baked == Some(false)
                && let Some(opacity) = view.number("opacity").filter(|n| *n < 1.)
            {
                string_prop(props, "opacity", format_number(opacity));
            }
        }
        // An export carries the node's own opacity: Figma writes it into the
        // SVG as `<g opacity>` and into a PNG's alpha. Written on the element
        // as well it is applied twice - a decoration at 0.2 came out at 0.04,
        // which is nothing, and the landing page's hero at 0.8 came out at
        // 0.64. The plugin writes it twice too. A mask is the same: the
        // SVG's own opacity thins the mask, so the colour painted through it
        // already shows at the node's opacity.
        push_blend_mode(&view, props);
        return derivations;
    }

    push_object_fit(&view, props, &mut derivations);
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
    if (explicit_button || named_button || component.eq_ignore_ascii_case("button"))
        && !props.iter().any(|(name, _)| name == "bg")
    {
        string_prop(props, "bg", "transparent");
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
        push_strokes(
            &view,
            single_outside_stroke(&view),
            props,
            used_tokens,
            variable_tokens,
        );
    }
    push_effects(
        &view,
        component,
        props,
        used_tokens,
        variable_tokens,
        lengths,
    );
    if let Some(opacity) = view.number("opacity")
        && opacity < 1.0
    {
        string_prop(props, "opacity", format_number(opacity));
    }
    push_blend_mode(&view, props);
    derivations
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
fn push_object_fit(
    view: &TypedNode<'_>,
    props: &mut Vec<Prop>,
    derivations: &mut StyleDerivations,
) {
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
        "FIT" => derivations.push(
            props,
            "objectFit",
            "contain",
            StyleDerivation::ImagePaintScale,
        ),
        "CROP" => derivations.push(
            props,
            "objectFit",
            "cover",
            StyleDerivation::ImagePaintScale,
        ),
        _ => {}
    }
}

fn push_blend_mode(view: &TypedNode<'_>, props: &mut Vec<Prop>) {
    let Some(blend) = view.string("blendMode") else {
        return;
    };
    let css = blend_mode(blend);
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
