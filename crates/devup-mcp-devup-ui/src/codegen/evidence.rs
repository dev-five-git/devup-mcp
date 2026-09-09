use std::collections::BTreeSet;

use devup_mcp_figma::Snapshot;
use serde_json::{Value, json};

use super::{CodegenOptions, CodegenOutput, style};

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
    if parent.is_some() && !absolute && !positioned {
        return None;
    }
    let relative = tag.contains("pos=\"relative\"");
    let embedded = options.root_layout == super::RootLayout::Embedded;
    let mut requirements = Vec::new();
    if absolute {
        requirements.push("Provide a positioned host matching the original parent dimensions and coordinate origin; the parent is not included in this component. Review the emitted offsets before insertion.".to_owned());
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
                requirements.push(format!("Ensure the generated root's {axis} matches the intended containing block (captured {axis}: {size}); this axis is determined by host CSS or in-flow content, not a generated fixed size."));
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
            "coordinateBasis": if absolute { "Emitted offsets resolve against the external positioned host; source parent placement is not self-contained." } else { "The root is inserted in normal document flow; source canvas x/y are not its placement in the application. Positioned children resolve against the generated root when it establishes a containing block." },
            "sourceSize": {"width":view.number("width"), "height":view.number("height"), "horizontal":view.string("layoutSizingHorizontal"), "vertical":view.string("layoutSizingVertical")},
            "sourcePosition": {"x":view.number("x"), "y":view.number("y"), "constraints":view.value("constraints")},
            "sourceParentSize": parent.map(|p| json!({"width":p.typed_view().number("width"), "height":p.typed_view().number("height")})),
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
                    "classification":classification,"appliedValueReason":reason,"nextAction":action});
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
