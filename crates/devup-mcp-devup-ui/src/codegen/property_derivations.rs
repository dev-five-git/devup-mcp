//! Source-stage derivations for properties not represented by the legacy map.
use std::collections::{BTreeMap, BTreeSet};

use devup_mcp_figma::{RawNode, Snapshot};
use serde_json::{Value, json};

use super::{
    CodegenOptions, animation,
    component::{Prop, PropValue, render_static_attribute},
    layout, style, text,
};

pub(crate) fn property_derivations(
    snapshot: &Snapshot,
    node: &RawNode,
    component: &str,
    options: &CodegenOptions,
    is_root: bool,
) -> Vec<Value> {
    let mut props = Vec::new();
    let mut origins = BTreeMap::new();
    let mut tokens = BTreeSet::new();
    let mut style_derivations = style::StyleDerivations::default();
    for stage in ["layout", "style", "text", "animation"] {
        let before = props.clone();
        match stage {
            "layout" => layout::push_layout_props(
                snapshot,
                node,
                component,
                &mut props,
                options.root_layout,
                is_root,
                options.width_policy.as_ref(),
                super::image::original_image(node, &options.asset_policies),
            ),
            "style" => {
                style_derivations = style::push_style_props(
                    snapshot,
                    node,
                    component,
                    super::image::asset_kind(snapshot, node, &options.asset_policies),
                    &mut props,
                    &mut tokens,
                    style::StyleOptions {
                        reserve_image_content: !(is_root
                            && options.root_layout == super::RootLayout::Embedded),
                        asset_policies: &options.asset_policies,
                        variable_tokens: &options.variable_tokens,
                        length_tokens: &options.length_tokens,
                        asset_names_per_node: options.asset_names_per_node,
                        width_policy: options.width_policy.as_ref(),
                    },
                );
            }
            "text" => text::push_text_props(
                &node.typed_view(),
                &options.text_style_tokens,
                &options.typography_values,
                &options.variable_tokens,
                &mut tokens,
                &mut props,
            ),
            "animation" => animation::push_animation_props(
                snapshot,
                node,
                &options.variable_tokens,
                &mut props,
            ),
            _ => unreachable!(),
        }
        for (name, value) in &props {
            if !before.iter().any(|p: &Prop| p.0 == *name && p.1 == *value) {
                origins.insert(name.clone(), stage);
            }
        }
    }
    // Written as render_node writes them, after the stages: a length bound to a
    // variable becomes its token in place, and the prop keeps the stage that
    // made it.
    style::tokenize_lengths(
        snapshot,
        node,
        &mut props,
        &options.length_tokens,
        &mut tokens,
    );
    let in_reading_grid = layout::in_reading_grid(snapshot, node);
    let in_wrapping_row = layout::in_wrapping_row(snapshot, node);
    props.into_iter().filter_map(|(name, PropValue::String(value))| {
        let stage = origins.get(&name)?;
        let (fields, calculation): (Vec<&str>, &str) = match name.as_str() {
            "data-navigation-width" => (vec!["name", "layoutMode", "childrenIds", "itemSpacing", "width"],
                "navigation_width: measured visible child widths plus row gaps expose a header/nav/GNB content-fit requirement to export and validation."),
            "w" | "h" | "maxW" | "minH" | "flexWrap" | "flexShrink" if in_wrapping_row =>
                (vec!["layoutMode", "layoutSizingHorizontal", "width", "height", "childrenIds", "parentId", "itemSpacing"],
                "fixed_row_props: wrap fixed items in available width, preserve drawn item widths/gaps, bound oversized descendants and let row height follow content."),
            "w" | "h" | "maxW" | "minW" | "gap" | "gridTemplateColumns" | "gridTemplateRows" | "gridColumn" | "gridRow"
                if in_reading_grid =>
                (vec!["absoluteBoundingBox", "childrenIds", "parentId", "layoutMode", "characters"],
                "reading_grid: contiguous numbered cards from transparent vertical column wrappers sorted into DOM sequence; measured bounds define grid tracks and placement at wide widths, mobile-first arrays reset to one column and auto rows. Descendants are bounded by their card width."),
            "flexShrink" => (vec!["layoutSizingHorizontal", "layoutSizingVertical", "layoutGrow", "parentId", "width", "height"],
                "push_layout_props: preserve the fixed primary-axis size in the parent flex layout by disabling shrink."),
            "wordBreak" => (vec!["styledTextSegments"], "push_text_props: Korean segment text uses keep-all word breaking."),
            "boxShadow" if style::asset_kind(snapshot, node).is_none() && style::single_outside_stroke(&node.typed_view()).is_some() =>
                (vec!["strokes", "strokeAlign", "strokeTopWeight", "strokeRightWeight", "strokeBottomWeight", "strokeLeftWeight", "effects"],
                include_str!("../text/property_derivations-property_derivations-f4103b4e.md")),
            "boxShadow" | "textShadow" | "filter" | "backdropFilter" => (vec!["effects"],
                include_str!("../text/property_derivations-property_derivations-a4bd5441.md")),
            "objectFit" | "objectPos" | "maskRepeat" | "maskSize" | "maskPos" => {
                if *stage != "style" { return None; }
                style_derivations.matching(&name, &value)?.description()
            },
            "alignSelf" => (vec!["layoutAlign", "layoutSizingHorizontal", "layoutSizingVertical", "parentId"], "Layout cross-axis fill maps to alignSelf=stretch when required by parent alignment."),
            "transform" | "transformOrigin" => (vec!["rotation", "relativeTransform", "absoluteBoundingBox", "absoluteRenderBounds", "constraints", "x", "y", "parentId"], include_str!("../text/property_derivations-property_derivations-691f8d10.md")),
            "zIndex" => (vec!["layoutPositioning", "fills", "childrenIds", "parentId"], "Layout stacking policy for absolute backgrounds and overlapping children, evaluated by push_layout_props."),
            "gridTemplateColumns" | "gridTemplateRows" =>
                (vec!["inferredAutoLayout", "gridColumnSizes", "gridRowSizes", "gridColumnCount", "gridRowCount", "layoutSizingHorizontal", "layoutSizingVertical"], include_str!("../text/property_derivations-property_derivations-a0f0555b.md")),
            "gridColumn" | "gridRow" | "columnGap" | "rowGap" =>
                (vec!["layoutMode", "gridColumnAnchorIndex", "gridRowAnchorIndex", "gridColumnSpan", "gridRowSpan", "gridColumnSizes", "gridRowSizes", "gridColumnCount", "gridRowCount", "gridColumnGap", "gridRowGap", "parentId"], "push_layout_props: grid tracks, one-based anchors/spans and grid gaps; parent grid count participates in placement."),
            "bgBlendMode" => (vec!["fills"], "push_style_props: visible paint blendMode maps to CSS background blend mode."),
            "mixBlendMode" => (vec!["blendMode"], "push_blend_mode converts the node blendMode into the generated CSS value."),
            "outline" | "outlineOffset" => (vec!["strokes", "strokeWeight", "strokeAlign"], "push_strokes composes stroke paints and alignment into outline and offset."),
            "bgClip" | "WebkitTextFillColor" => (vec!["fills"], "Text gradient paint is clipped to text with transparent text fill."),
            "visibility" | "display" => (vec!["visible", "opacity", "absoluteRenderBounds", "fills", "textTruncation", "maxLines"], "Visibility/non-rendering or text truncation policy in the named stage; existing visibility classification is retained."),
            "WebkitTextStroke" | "paintOrder" => (vec!["strokes", "strokeWeight", "styledTextSegments"], "push_text_props composes text stroke width/color and stroke-before-fill paint order."),
            "WebkitBoxOrient" | "WebkitLineClamp" | "textOverflow" => (vec!["textTruncation", "maxLines", "styledTextSegments"], "push_text_props projects truncation and maximum line count into the emitted clamp properties."),
            "whiteSpace" if text::preserves_hard_break_spaces(&node.typed_view()) => (vec!["characters", "styledTextSegments", "textAutoResize", "maxLines"], include_str!("../text/property_derivations-property_derivations-2ec1244f.md")),
            "textDecoration" => (vec!["textDecoration", "styledTextSegments"], "Text UNDERLINE/STRIKETHROUGH maps to underline/line-through."),
            "textTransform" => (vec!["textCase", "styledTextSegments"], "Text case projection follows push_text_props."),
            "as" | "my" => (vec!["styledTextSegments", "listOptions", "paragraphSpacing"], "Text list/paragraph projection uses the emitted semantic tag and spacing policy."),
            "animationName" | "animationDuration" | "animationTimingFunction" | "animationFillMode" | "animationDelay" | "animationIterationCount" =>
                (vec!["reactions", "relativeTransform", "rotation", "childrenIds"], include_str!("../text/property_derivations-property_derivations-f0db063f.md")),
            // No generic 'the renderer wrote it' proof. A newly introduced
            // property must register its source fields/calculation or receives
            // PROPERTY_UNMAPPED from the final AST audit.
            _ => return None,
        };
        let source_fields: Vec<_> = fields.iter().map(|field| if name == "wordBreak" && *field == "styledTextSegments" { "styledTextSegments[].characters" } else { field }).collect();
        let original: BTreeMap<_, _> = fields.iter().filter_map(|field| node.typed_view().value(field).map(|v| {
            if name == "wordBreak" && *field == "styledTextSegments" {
                ("styledTextSegments[].characters", json!(v.as_array().into_iter().flatten().filter_map(|s|s.get("characters")).collect::<Vec<_>>()))
            } else { (*field, v.clone()) }
        })).collect();
        // Retain parent geometry when the stage can consult it, not the entire
        // collected subtree or asset bytes in each property record.
        let parent = node.typed_view().string("parentId").and_then(|id| snapshot.nodes.get(id));
        let mut context = serde_json::Map::new();
        if *stage == "layout" {
            context.insert("rootLayout".into(), json!(options.root_layout));
            context.insert("isRenderRoot".into(), json!(is_root));
        }
        if *stage != "text" && let Some(parent) = parent {
            context.insert("parentNodeId".into(), json!(parent.id));
            if let Some(bounds) = parent.typed_view().value("absoluteBoundingBox") {
                context.insert("parentBounds".into(), bounds.clone());
            }
        }
        let bound: BTreeMap<_,_> = options.variable_tokens.iter().filter(|(_,token)|value.contains(&format!("${token}"))).collect();
        if !bound.is_empty() { context.insert("variableTokens".into(), json!(bound)); }
        let mut evidence = json!({"stage":stage,
            "generatedProperty":render_static_attribute(&name, &value),
            "sourceFields":source_fields,"originalValue":original,"calculation":calculation});
        if let Some(route) = style_derivations.matching(&name, &value) {
            evidence["derivationPath"] = json!(route.path());
        }
        if !context.is_empty() { evidence["context"] = Value::Object(context); }
        Some(evidence)
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r15_image_fit_evidence_tracks_paint_or_boundary_writer() {
        for (positioning, value, path, prefix) in [
            (
                "ABSOLUTE",
                "fill",
                "native-raster-boundary",
                "Native raster boundary:",
            ),
            (
                "AUTO",
                "none",
                "in-flow-export-boundary",
                "In-flow export boundary:",
            ),
        ] {
            let snapshot: Snapshot = serde_json::from_value(json!({
                "fileKey":"r15-image","roots":["p"],"diagnostics":[],"nodes":{
                    "p":{"id":"p","type":"FRAME","fields":{"layoutMode":"HORIZONTAL","childrenIds":["c"]}},
                    "c":{"id":"c","type":"RECTANGLE","fields":{
                        "parentId":"p","isAsset":true,"layoutPositioning":positioning,
                        "absoluteBoundingBox":{"x":10,"y":20,"width":100,"height":50},
                        "absoluteRenderBounds":{"x":12,"y":23,"width":80,"height":40},
                        "fills":[{"type":"IMAGE","visible":true,"scaleMode":"FIT"}]}}
                }
            })).unwrap();
            let evidence = property_derivations(
                &snapshot,
                &snapshot.nodes["c"],
                "Image",
                &CodegenOptions {
                    // This regression covers the explicit native export boundary.
                    asset_policies: BTreeMap::from([(
                        "c".into(),
                        super::super::AssetRepresentationPolicy {
                            render_mode: devup_mcp_figma::AssetRenderMode::Node,
                            ..Default::default()
                        },
                    )]),
                    ..Default::default()
                },
                false,
            );
            let fit = evidence
                .iter()
                .find(|e| e["generatedProperty"] == format!("objectFit=\"{value}\""))
                .unwrap();
            assert!(
                fit["calculation"].as_str().unwrap().starts_with(prefix),
                "{fit}"
            );
            assert_eq!(fit["derivationPath"], path);
            if positioning == "AUTO" {
                let pos = evidence
                    .iter()
                    .find(|e| e["generatedProperty"] == "objectPos=\"2px 3px\"")
                    .unwrap();
                assert_eq!(pos["derivationPath"], path);
            }
        }
    }
}
