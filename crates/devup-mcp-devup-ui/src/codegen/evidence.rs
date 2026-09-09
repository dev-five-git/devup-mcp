use std::collections::BTreeSet;

use devup_mcp_figma::Snapshot;
use serde_json::{Value, json};

use super::{CodegenOptions, CodegenOutput, style};

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
