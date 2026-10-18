use std::collections::BTreeSet;

use devup_mcp_figma::Snapshot;
use serde_json::{Value, json};

use super::{CodegenOptions, CodegenOutput, style};

/// Report uncertainty using emitted layout and acquired pixel evidence.
/// A measured node frame alone never proves intrinsic source dimensions.
pub(super) fn image_layout_diagnostics(
    snapshot: &Snapshot,
    output: &CodegenOutput,
) -> Vec<devup_mcp_figma::Diagnostic> {
    use crate::provenance::attributes::{OwnedElement, owned_elements};
    use devup_mcp_figma::{Diagnostic, DiagnosticSeverity, FidelityImpact};

    let elements = owned_elements(output);
    let sizing = |element: &OwnedElement| {
        element
            .attributes
            .iter()
            .filter(|(a, _)| {
                matches!(
                    a.name.as_str(),
                    "w" | "h"
                        | "boxSize"
                        | "aspectRatio"
                        | "minW"
                        | "minH"
                        | "maxW"
                        | "maxH"
                        | "flex"
                        | "flexShrink"
                        | "gridTemplateColumns"
                        | "gridTemplateRows"
                )
            })
            .map(|(a, _)| a.source.clone())
            .collect::<Vec<_>>()
    };
    let mut diagnostics = Vec::new();
    for element in &elements {
        let Some(node) = element
            .node_id
            .as_ref()
            .and_then(|id| snapshot.nodes.get(id))
        else {
            continue;
        };
        if element.tag == "Grid" {
            for (columns, property, sizes, count) in [
                (
                    true,
                    "gridTemplateColumns",
                    "gridColumnSizes",
                    "gridColumnCount",
                ),
                (false, "gridTemplateRows", "gridRowSizes", "gridRowCount"),
            ] {
                let template = super::layout::grid_template(node, columns);
                if template.unverified.is_empty() {
                    continue;
                }
                let generated = element
                    .attributes
                    .iter()
                    .find(|(a, _)| a.name == property)
                    .map(|(a, _)| &a.source);
                diagnostics.push(Diagnostic {
                    code: "DEVUP_CODEGEN_GRID_TRACK_SIZING_UNVERIFIED".into(),
                    message: "Grid track sizing metadata is missing or ambiguous; review the emitted tracks and sizing policy.".into(),
                    node_id: Some(node.id.clone()), property: Some(property.into()),
                    severity: Some(DiagnosticSeverity::Warning),
                    fidelity_impact: Some(FidelityImpact::Approximated),
                    details: Some(json!({
                        "classification":"grid-track-sizing", "reasonCodes":template.unverified,
                        "originalValue":{"sizes":node.typed_view().value(sizes),"count":node.typed_view().value(count)},
                        "generatedProperty":generated, "generatedSizing":sizing(element),
                        "sizingPolicy":include_str!("../text/evidence-image_layout_diagnostics_Diagnostic-c94c1150.md"),
                        "verification":{"state":"unverified","reasonCode":"source-track-sizing-incomplete"},
                        "nextAction":"Collect explicit track sizes and child FIXED/FILL/HUG sizing. Compare computed card widths before accepting the fallback; no original card width is inferred.",
                        "evidenceLimit":"Generated CSS policy and collected metadata only; browser layout was not measured."
                    })),
                    ..Diagnostic::default()
                });
            }
        }
        if element.tag != "Image" {
            continue;
        }
        let fixed_axis = |prop: &str| {
            element.attributes.iter().any(|(a, _)| {
                (a.name == prop || a.name == "boxSize")
                    && a.source
                        .split_once("=\"")
                        .and_then(|(_, v)| v.strip_suffix('"'))
                        .and_then(|v| v.strip_suffix("px"))
                        .and_then(|v| v.parse::<f64>().ok())
                        .is_some_and(|v| v.is_finite() && v >= 0.0)
            })
        };
        if fixed_axis("w") && fixed_axis("h") {
            continue;
        }
        let has_size = |prop: &str| {
            element.attributes.iter().any(|(a, _)| {
                (a.name == prop || a.name == "boxSize") && !a.source.ends_with("=\"auto\"")
            })
        };
        let has_ratio = element
            .attributes
            .iter()
            .any(|(a, _)| a.name == "aspectRatio");
        let reserved_shape =
            (has_size("w") && has_size("h")) || (has_ratio && (has_size("w") || has_size("h")));
        let mut current = node;
        let mut seen = BTreeSet::new();
        while seen.insert(current.id.as_str()) {
            let Some(parent) = current
                .typed_view()
                .string("parentId")
                .and_then(|id| snapshot.nodes.get(id))
                .or_else(|| {
                    snapshot
                        .nodes
                        .values()
                        .find(|p| p.typed_view().child_ids().any(|id| id == current.id))
                })
            else {
                break;
            };
            // Absolute images do not contribute to a flex/grid track's
            // min-content size. A hidden/component-folded ancestor is likewise
            // outside this generated layout's observable contents.
            if elements.iter().any(|e| {
                e.node_id.as_deref() == Some(&current.id)
                    && e.attributes
                        .iter()
                        .any(|(a, _)| a.source == "pos=\"absolute\"")
            }) {
                break;
            }
            let Some(parent_element) = elements
                .iter()
                .find(|e| e.node_id.as_deref() == Some(&parent.id))
            else {
                break;
            };
            // Component selection follows inferred layout. A raw source mode
            // alone does not prove that a Box/component reference is CSS flex.
            let mode = match parent_element.tag.as_str() {
                "Grid" => Some("GRID"),
                "Flex" => Some("HORIZONTAL"),
                "VStack" => Some("VERTICAL"),
                "Center"
                    if parent_element
                        .attributes
                        .iter()
                        .any(|(a, _)| a.source == "flexDir=\"column\"") =>
                {
                    Some("VERTICAL")
                }
                "Center" => Some("HORIZONTAL"),
                _ => None,
            };
            let columns = parent_element
                .attributes
                .iter()
                .find(|(a, _)| a.name == "gridTemplateColumns")
                .map(|(a, _)| a.source.as_str());
            let rows = parent_element
                .attributes
                .iter()
                .find(|(a, _)| a.name == "gridTemplateRows")
                .map(|(a, _)| a.source.as_str());
            let content_sized = |css: Option<&str>| {
                css.is_none_or(|css| {
                    css.contains("fr") || css.contains("auto") || css.contains("fit-content")
                })
            };
            let flexible_grid =
                mode == Some("GRID") && (content_sized(columns) || content_sized(rows));
            let bounded_columns = columns.is_some_and(|css| {
                !css.contains("auto")
                    && !css.contains("fit-content")
                    && (!css.contains("fr") || css.contains("minmax(0,"))
            });
            if mode == Some("GRID") && bounded_columns && reserved_shape {
                // Width/height or a source-supported ratio already reserve the
                // image box here. No decoded image dimension determines it.
                break;
            }
            if flexible_grid || matches!(mode, Some("HORIZONTAL" | "VERTICAL")) {
                let view = node.typed_view();
                diagnostics.push(Diagnostic {
                    code: "DEVUP_CODEGEN_IMAGE_INTRINSIC_LAYOUT".into(),
                    message: "Intrinsic image dimensions may affect content sizing or overflow in this flexible layout; compare computed card widths and heights.".into(),
                    node_id: Some(node.id.clone()), property: Some("imageSizing".into()),
                    severity: Some(DiagnosticSeverity::Warning),
                    fidelity_impact: Some(FidelityImpact::Approximated),
                    details: Some(json!({
                        "classification":"intrinsic-image-layout",
                        "nodeSize":{"width":view.number("width"),"height":view.number("height"),"horizontal":view.string("layoutSizingHorizontal"),"vertical":view.string("layoutSizingVertical")},
                        "sourceImageSize":output.source_map.asset_css_mappings.iter().find(|m|m["nodeId"]==node.id).map(|m|&m["sourcePixelSize"]),
                        "outputImageSize":output.source_map.asset_css_mappings.iter().find(|m|m["nodeId"]==node.id).map(|m|&m["outputPixelSize"]),
                        "intrinsicAttributes":element.attributes.iter().filter(|(a,_)|matches!(a.name.as_str(),"width"|"height")).map(|(a,_)|a.source.clone()).collect::<Vec<_>>(),
                        "sourceImageSizeReason":"Pixel sizes come only from supplied acquisition evidence; node dimensions alone describe the layout frame.",
                        "generatedSizing":sizing(element),
                        "layout":{"nodeId":parent.id,"mode":mode,"generatedSizing":sizing(parent_element),"zeroMinimumColumns":columns.is_some_and(|css| css.contains("minmax(0,"))},
                        "verification":{"state":"unverified","reasonCode":"browser-intrinsic-layout-not-measured"},
                        "nextAction":include_str!("../text/evidence-image_layout_diagnostics_Diagnostic-79e776df.md"),
                        "evidenceLimit":"Potential CSS intrinsic-size dependency, not a measured cause or proof of unequal cards. Intrinsic attributes use acquired pixels when available."
                    })),
                    ..Diagnostic::default()
                });
                break;
            }
            current = parent;
        }
    }
    diagnostics
}

/// Describe the emitted root's placement separately from property coverage.
/// The selected root's parent is never rendered by this component, even when
/// that parent was collected as part of a larger snapshot.
pub(super) fn placement_contract(
    snapshot: &Snapshot,
    output: &CodegenOutput,
    options: &CodegenOptions,
    root_id: &str,
) -> Option<devup_mcp_figma::Diagnostic> {
    use devup_mcp_figma::{Diagnostic, DiagnosticSeverity, FidelityImpact};
    let selected = snapshot.nodes.get(root_id)?;
    let node = if selected.node_type == "SECTION" {
        selected
            .typed_view()
            .child_ids()
            .next()
            .and_then(|id| snapshot.nodes.get(id))
            .unwrap_or(selected)
    } else {
        selected
    };
    let root_id = node.id.as_str();
    let view = node.typed_view();
    if view.bool("visible") == Some(false) {
        return None;
    }
    let parent = view
        .string("parentId")
        .and_then(|id| snapshot.nodes.get(id));
    // The parent's own paint, when the capture holds it. A screen exported on
    // its own showed nothing of the surface it is drawn on, so learning that
    // the desktop frame behind an app screen was #E6EDE6 took a second export
    // of that parent — a second Figma acquisition to read a colour this
    // capture was already carrying.
    let parent_background = parent.and_then(|parent| {
        let parent = parent.typed_view();
        let color = style::first_solid_color(parent.value("fills"))?;
        Some(json!({
            "color": color,
            "layoutMode": parent.string("layoutMode"),
            "name": parent.string("name"),
            "note": include_str!("../text/evidence-placement_contract-5c61c6c3.md"),
        }))
    });
    let source = output
        .source_map
        .entries
        .iter()
        .find(|e| e.node_id.as_deref() == Some(root_id) && e.property.is_none())
        .and_then(|e| e.generated_range.as_ref())
        .and_then(|r| output.tsx.get(r.start..r.end))?;
    let tag = source.split_once('>')?.0;
    let absolute = tag.contains("pos=\"absolute\"");
    let positioned = super::layout::holds_positioned_children(snapshot, node);
    let vertical_fill = super::layout::vertical_fill_container(snapshot, node);
    // An in-flow root inside a collected parent needs no placement contract —
    // unless that parent paints the surface underneath it, which nothing else
    // in the response would say.
    if parent.is_some() && !absolute && !positioned && !vertical_fill && parent_background.is_none()
    {
        return None;
    }
    let relative = tag.contains("pos=\"relative\"");
    let embedded = options.root_layout == super::RootLayout::Embedded;
    let mut requirements = Vec::new();
    if absolute {
        requirements
            .push(include_str!("../text/evidence-placement_contract-adba181a.md").to_owned());
    }
    if vertical_fill
        && (embedded
            || !view.number("height").is_some_and(|h| {
                tag.contains(&format!("h=\"{}\"", super::layout::px(h)))
                    || tag.contains(&format!("boxSize=\"{}\"", super::layout::px(h)))
            }))
    {
        requirements.push(include_str!("../text/evidence-placement_contract-7c9ff723.md").into());
    }
    if positioned {
        for (axis, sizing, prop) in [
            ("width", "layoutSizingHorizontal", "w"),
            ("height", "layoutSizingVertical", "h"),
        ] {
            let captured = view.number(axis);
            let emitted = captured.is_some_and(|value| {
                let px = super::layout::px(value);
                tag.contains(&format!("{prop}=\"{px}\""))
                    || tag.contains(&format!("boxSize=\"{px}\""))
            });
            if embedded || view.string(sizing) != Some("FIXED") || !emitted {
                let size = captured
                    .map(super::layout::px)
                    .unwrap_or_else(|| "not collected".into());
                requirements.push(format!(
                    include_str!("../text/evidence-placement_contract-09b061a1.md"),
                    axis = axis,
                    size = size
                ));
            }
        }
        if !absolute && !relative {
            requirements.push("The emitted component reference must implement the original frame's containing block and dimensions; its internal CSS is not included.".into());
        }
    }
    let dependent = !requirements.is_empty();
    Some(Diagnostic {
        code: "DEVUP_CODEGEN_PLACEMENT_CONTRACT".into(),
        message: "Root placement contract: inspect the coordinate basis and host requirements before inserting this TSX.".into(),
        node_id: Some(root_id.into()), property: Some("placement".into()),
        severity: Some(if dependent { DiagnosticSeverity::Warning } else { DiagnosticSeverity::Info }),
        fidelity_impact: Some(if dependent { FidelityImpact::Approximated } else { FidelityImpact::None }),
        details: Some(json!({
            "rootLayout": options.root_layout,
            "parentId": view.string("parentId"), "parentCollected": parent.is_some(),
            "parentIncludedInOutput": false,
            "parentMissingReason": if parent.is_none() { Some("The root is at the capture boundary; its parent geometry was not collected.") } else { None },
            "containingBlock": if absolute { "external-host" } else if relative { "generated-root" } else { "normal-flow-host" },
            "coordinateBasis": if absolute { "Emitted offsets resolve against the external positioned host; source parent placement is not self-contained." } else { include_str!("../text/evidence-placement_contract_Diagnostic-d84874e6.md") },
            "sourceSize": {"width":view.number("width"), "height":view.number("height"), "horizontal":view.string("layoutSizingHorizontal"), "vertical":view.string("layoutSizingVertical")},
            "sourcePosition": {"x":view.number("x"), "y":view.number("y"), "constraints":view.value("constraints")},
            "sourceParentSize": parent.map(|p| json!({"width":p.typed_view().number("width"), "height":p.typed_view().number("height")})),
            "sourceParentBackground": parent_background,
            "generatedRoot": format!("{tag}>"), "hostRequirements":requirements,
            "classification":"placement-contract",
            "nextAction": if dependent { "Satisfy hostRequirements or export the containing parent in standalone mode before accepting placement." } else { "Insert in normal flow using the emitted root props. External parent placement is outside this capture." },
            "evidenceLimit":"This is a generated CSS placement contract, not a measured browser or responsive equivalence result."
        })),
        ..Diagnostic::default()
    })
}

/// Generated evidence is not proof that a Figma property was preserved. In
/// particular, an instance reference does not prove its implementation's CSS.
pub(super) fn uncovered_layout_details(
    snapshot: &Snapshot,
    output: &CodegenOutput,
    options: &CodegenOptions,
    node_id: &str,
    property: &str,
) -> Value {
    let original = snapshot
        .nodes
        .get(node_id)
        .and_then(|n| n.typed_view().value(property));
    let mut owner = Some(node_id.to_owned());
    let mut seen = BTreeSet::new();
    while let Some(id) = owner {
        if !seen.insert(id.clone()) {
            break;
        }
        let node = snapshot.nodes.get(&id);
        let component =
            !options.inline_instances && node.is_some_and(|n| n.node_type == "INSTANCE");
        let asset = node.is_some_and(|n| style::asset_kind(snapshot, n).is_some());
        if id == node_id || component || asset {
            let entry = output.source_map.entries.iter().find(|e| {
                e.node_id.as_deref() == Some(&id)
                    && e.property.is_none()
                    && e.resolution == "node"
                    && e.generated_range.as_ref().is_some_and(|r| r.start < r.end)
            });
            if let Some(range) = entry.and_then(|e| e.generated_range.as_ref())
                && let Some(source) = output.tsx.get(range.start..range.end)
            {
                let auto_text = id == node_id
                    && matches!(property, "width" | "height")
                    && node.is_some_and(|n| {
                        n.node_type == "TEXT"
                            && n.typed_view().string("textAutoResize") == Some("WIDTH_AND_HEIGHT")
                            && n.typed_view().string("layoutSizingHorizontal") == Some("FIXED")
                            && n.typed_view().string("layoutSizingVertical") == Some("FIXED")
                    });
                let (classification, reason, action) = if auto_text {
                    (
                        "text-auto-size",
                        "textAutoResize=WIDTH_AND_HEIGHT makes the generator omit fixed width and height; emitted typography and content determine the browser size.",
                        "Review the emitted Text and font metrics. The generator uses content sizing; the source width/height are measurements, not emitted fixed CSS dimensions.",
                    )
                } else if component {
                    (
                        "component-reference",
                        "The generator emitted a component reference; this output does not contain the component's internal layout implementation.",
                        "Review this component implementation or use the inline tsx output for this frame; do not infer its internal CSS from the reference.",
                    )
                } else if asset {
                    (
                        "asset-projection",
                        "The generator emitted an asset element; the source field has no verified individual layout mapping in this output.",
                        "Review the generated asset element's sizing and the exported asset; internal asset geometry is not verified by this diagnostic.",
                    )
                } else {
                    (
                        "property-unmapped",
                        "The node was emitted, but no verified property mapping accounts for this source layout field.",
                        "Compare this generated element's layout props with the source field; the excerpt reports actual code, not computed browser dimensions.",
                    )
                };
                // A bounded opening excerpt identifies the actual replacement
                // without copying entire screens into every layout issue.
                let excerpt: String = source.chars().take(800).collect();
                return json!({"originalValue":original,
                    "originalValueReason":if original.is_none() { Some("The collected node has no value for this property.") } else { None },
                    "appliedValue":{"state":"emitted","generatedNodeId":id,"generatedSource":excerpt,
                        "sourceTruncated":source.chars().count()>800,"propertyMappingVerified":false},
                    "classification":classification,"appliedValueReason":reason,"nextAction":action,
                    "verification":if auto_text {json!({"state":"unverified","reasonCode":"font-metrics-not-measured","reason":"Content sizing was emitted, but browser font metrics and measured dimensions have not been verified; this remains lossy, not exact."})} else {Value::Null},
                    "implicitCssVerification":crate::provenance::implicit_css_verification(snapshot, output, node_id, property)});
            }
        }
        owner = node
            .and_then(|n| n.typed_view().string("parentId"))
            .map(str::to_owned)
            .or_else(|| {
                snapshot
                    .nodes
                    .values()
                    .find(|n| n.typed_view().child_ids().any(|child| child == id))
                    .map(|n| n.id.clone())
            });
    }
    let trace = output
        .projection_trace
        .entries
        .iter()
        .find(|e| e.node_id == node_id);
    json!({"originalValue":original,
        "originalValueReason":if original.is_none() { Some("The collected node has no value for this property.") } else { None },
        "appliedValue":{"state":"no-mapped-source","generatedNodeId":null,"generatedSource":null,
            "propertyMappingVerified":false,"traceReason":trace.map(|t| &t.reason)},
        "classification":"unclassified",
        "appliedValueReason":format!("No generated range exists for {node_id}#{property} or an enclosing asset/component replacement; trace={}",trace.map(|t| t.reason.as_str()).unwrap_or("absent")),
        "nextAction":"Inspect the source node and its projection trace; no computed CSS value can be inferred from absent source mapping."})
}

/// Original coordinates must travel with the approximation, not just its enum.
/// Bounding-box deltas are labelled explicitly: rotated ancestors need an
/// inverse transform to recover local coordinates, so never call those exact x/y.
pub(super) fn placement_evidence(snapshot: &Snapshot, node_id: &str) -> Value {
    let Some(node) = snapshot.nodes.get(node_id) else {
        return json!({"nodeId":node_id,"missingReason":"The source node was not collected."});
    };
    let parent = node
        .typed_view()
        .string("parentId")
        .and_then(|id| snapshot.nodes.get(id))
        .or_else(|| {
            snapshot
                .nodes
                .values()
                .find(|n| n.typed_view().child_ids().any(|id| id == node_id))
        });
    let mut original = geometry(node);
    original["parent"] = parent.map(geometry).unwrap_or(Value::Null);
    original["parentMissingReason"] = if parent.is_none() {
        json!("Parent was not collected; containing-block geometry cannot be verified.")
    } else {
        Value::Null
    };
    let view = node.typed_view();
    let bounds = view.value("absoluteBoundingBox");
    let parent_bounds = parent.and_then(|n| n.typed_view().value("absoluteBoundingBox"));
    let delta = |axis: &str| {
        bounds
            .and_then(|b| b[axis].as_f64())
            .zip(parent_bounds.and_then(|b| b[axis].as_f64()))
            .map(|(a, b)| a - b)
    };
    let (x, y) = (delta("x"), delta("y"));
    original["parentRelativeBounds"] = if let (Some(x), Some(y)) = (x, y) {
        json!({"x":x,"y":y,"width":bounds.and_then(|b|b.get("width")),"height":bounds.and_then(|b|b.get("height")),
            "basis":"absoluteBoundingBox offsets; axis-aligned canvas bounds, not inverse-transformed local coordinates"})
    } else if parent.is_some_and(|p| p.node_type != "GROUP")
        && view.number("x").is_some()
        && view.number("y").is_some()
    {
        json!({"x":view.number("x"),"y":view.number("y"),"width":view.number("width"),"height":view.number("height"),
            "basis":"collected x/y in the parent coordinate system"})
    } else {
        json!({"x":null,"y":null,"basis":"unavailable","reason":"Need both bounding boxes or unambiguous parent-local x/y."})
    };
    let mut children: Vec<_> = view.child_ids().collect();
    // derived_padding uses the only visible child; keep that source even if
    // many hidden siblings precede it in the design's child ordering.
    children.sort_by_key(|id| {
        snapshot
            .nodes
            .get(*id)
            .is_some_and(|n| n.typed_view().bool("visible") == Some(false))
    });
    original["children"] =
        json!(
            children
                .iter()
                .take(16)
                .map(|id| snapshot.nodes.get(*id).map(geometry).unwrap_or_else(
                    || json!({"nodeId":id,"missingReason":"Child was not collected."})
                ))
                .collect::<Vec<_>>()
        );
    original["childCount"] = json!(children.len());
    original["childrenTruncated"] = json!(children.len() > 16);
    original
}

fn geometry(node: &devup_mcp_figma::RawNode) -> Value {
    let view = node.typed_view();
    let mut value = json!({"nodeId":node.id,"nodeType":node.node_type});
    for field in [
        "parentId",
        "x",
        "y",
        "width",
        "height",
        "rotation",
        "relativeTransform",
        "absoluteTransform",
        "absoluteBoundingBox",
        "absoluteRenderBounds",
        "constraints",
        "layoutPositioning",
        "layoutMode",
        "layoutSizingHorizontal",
        "layoutSizingVertical",
        "layoutAlign",
        "layoutGrow",
        "inferredAutoLayout",
        "paddingTop",
        "paddingRight",
        "paddingBottom",
        "paddingLeft",
        "itemSpacing",
        "primaryAxisAlignItems",
        "counterAxisAlignItems",
        "minWidth",
        "maxWidth",
        "minHeight",
        "maxHeight",
        "clipsContent",
        "isMask",
        "maskType",
        "visible",
        "opacity",
        "effects",
        "fills",
    ] {
        value[field] = view.value(field).cloned().unwrap_or(Value::Null);
    }
    let missing: Vec<_> = [
        "x",
        "y",
        "width",
        "height",
        "constraints",
        "absoluteBoundingBox",
    ]
    .into_iter()
    .filter(|field| value[*field].is_null())
    .collect();
    value["missingFields"] = json!(missing);
    value["missingValueReason"] = json!(
        "Null means absent or null in the collected source; it is not a zero, default constraint, or verified mapping."
    );
    value
}

pub(super) fn fallback_original(snapshot: &Snapshot, node_id: &str, property: &str) -> Value {
    match property {
        "layoutPositioning" | "isMask" | "childrenIds" => placement_evidence(snapshot, node_id),
        _ => snapshot
            .nodes
            .get(node_id)
            .and_then(|n| n.typed_view().value(property))
            .cloned()
            .unwrap_or(Value::Null),
    }
}
