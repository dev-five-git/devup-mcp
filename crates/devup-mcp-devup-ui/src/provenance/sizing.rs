use super::*;
use serde_json::Value;

fn prop<'a>(opening: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let start = find_prop(opening, &needle)? + needle.len();
    opening[start..].split_once('"').map(|(v, _)| v)
}

fn flex_axis(opening: &str) -> Option<&'static str> {
    let tag = opening
        .trim_start()
        .strip_prefix('<')?
        .split(|c: char| c.is_whitespace() || c == '/')
        .next()?;
    if !matches!(tag, "VStack" | "Flex" | "Center") {
        return None;
    }
    match prop(opening, "flexDir") {
        Some("column") => Some("VERTICAL"),
        Some("row") => Some("HORIZONTAL"),
        None if tag == "VStack" => Some("VERTICAL"),
        None => Some("HORIZONTAL"),
        _ => None,
    }
}

fn parent<'a>(
    snapshot: &'a Snapshot,
    output: &CodegenOutput,
    id: &str,
) -> Option<&'a devup_mcp_figma::RawNode> {
    let parent = snapshot
        .nodes
        .values()
        .find(|p| p.typed_view().child_ids().any(|c| c == id))?;
    let range = output
        .source_map
        .entries
        .iter()
        .find(|e| e.node_id.as_deref() == Some(id) && e.property.is_none())?
        .generated_range
        .as_ref()?;
    let owner = output
        .source_map
        .entries
        .iter()
        .filter(|e| e.property.is_none() && e.node_id.as_deref() != Some(id))
        .filter_map(|e| Some((e.node_id.as_deref()?, e.generated_range.as_ref()?)))
        .filter(|(_, r)| r.start <= range.start && r.end >= range.end)
        .min_by_key(|(_, r)| r.end - r.start)?
        .0;
    (owner == parent.id).then_some(parent)
}

pub(super) fn has_vertical_flex_basis(
    snapshot: &Snapshot,
    output: &CodegenOutput,
    id: &str,
) -> bool {
    let Some(parent) = parent(snapshot, output, id) else {
        return false;
    };
    parent.typed_view().string("layoutMode") == Some("VERTICAL")
        && node_opening(output, &parent.id).is_some_and(|s| flex_axis(s) == Some("VERTICAL"))
        && node_opening(output, id).is_some_and(|s| {
            prop(s, "flex") == Some("1") && !matches!(prop(s, "pos"), Some("absolute" | "fixed"))
        })
        && snapshot
            .nodes
            .get(id)
            .is_some_and(|n| n.typed_view().string("layoutSizingVertical") == Some("FILL"))
}

/// The result is rechecked against emitted CSS during fidelity validation;
/// a resolution string or an arbitrary property range is never proof.
pub(crate) fn implicit_css_verification(
    snapshot: &Snapshot,
    output: &CodegenOutput,
    id: &str,
    field: &str,
) -> Value {
    let Some(node) = snapshot.nodes.get(id) else {
        return json!({"state":"not-verifiable","reason":"Source node was not collected."});
    };
    let Some(parent) = parent(snapshot, output, id) else {
        return json!({"state":"not-verifiable","reason":"The source parent is missing or is not the emitted immediate parent."});
    };
    let (Some(child_tag), Some(parent_tag)) =
        (node_opening(output, id), node_opening(output, &parent.id))
    else {
        return json!({"state":"not-verifiable","reason":"Generated parent or child CSS is unavailable."});
    };
    let Some(axis) = flex_axis(parent_tag) else {
        return json!({"state":"not-verifiable","parentId":parent.id,"reason":"The emitted parent is not a known flex primitive; its CSS cannot be verified."});
    };
    let sizing = match field {
        "width" => "layoutSizingHorizontal",
        "height" | "layoutSizingVertical" => "layoutSizingVertical",
        _ => {
            return json!({"state":"not-accounted-for","reason":"Flex stretch does not account for this field."});
        }
    };
    let width = field == "width";
    let view = node.typed_view();
    let source_mode = parent.typed_view().string("layoutMode");
    let cross = if width {
        axis == "VERTICAL"
    } else {
        axis == "HORIZONTAL"
    };
    let default_center = parent_tag.trim_start().starts_with("<Center");
    let aligned = match prop(child_tag, "alignSelf") {
        Some("stretch") => true,
        None | Some("auto") => {
            prop(parent_tag, "alignItems").map_or(!default_center, |v| v == "stretch")
        }
        _ => false,
    };
    let blocked = [
        if width { "w" } else { "h" },
        "boxSize",
        if width { "maxW" } else { "maxH" },
        if width { "minW" } else { "minH" },
        "m",
        "mx",
        "my",
        "ml",
        "mr",
        "mt",
        "mb",
    ]
    .iter()
    .any(|p| find_prop(child_tag, &format!("{p}=")).is_some());
    let accounted = cross
        && source_mode == Some(axis)
        && view.string(sizing) == Some("FILL")
        && !matches!(prop(child_tag, "pos"), Some("absolute" | "fixed"))
        && view.string("layoutPositioning") != Some("ABSOLUTE")
        && aligned
        && !blocked
        && fill_axis_is_established(snapshot, output, id, if width { "width" } else { "height" });
    json!({"state":if accounted {"accounted-for"} else {"not-accounted-for"},
        "parentId":parent.id,"parentLayoutMode":source_mode,"childSizing":view.string(sizing),
        "generatedParent":parent_tag,"generatedChild":child_tag,
        "cssBehavior":"flex cross-axis align-items: stretch (or align-self: stretch)",
        "reason":if accounted {"The emitted immediate flex parent stretches this in-flow FILL child across an established cross axis; no explicit size, margin or min/max constraint overrides stretch."} else {"Checked emitted flex direction, FILL sizing, alignment, positioning, explicit dimensions, margins, constraints and parent size basis; these do not prove implicit stretch."}})
}

pub(super) fn sizing_mapping_matches(
    snapshot: &Snapshot,
    output: &CodegenOutput,
    id: &str,
    field: &str,
    source: &str,
) -> bool {
    let Some(child_tag) = node_opening(output, id) else {
        return false;
    };
    let Some(node) = snapshot.nodes.get(id) else {
        return false;
    };
    let view = node.typed_view();
    if field == "layoutSizingVertical" && view.string(field) == Some("FIXED") {
        return view.number("height").is_some_and(|h| {
            let h = crate::codegen::px(h);
            source == format!("h=\"{h}\"") || source == format!("boxSize=\"{h}\"")
        });
    }
    if field == "layoutSizingVertical"
        && view.string(field) == Some("FILL")
        && matches!(source, "h=\"100%\"" | "boxSize=\"100%\"")
        && fill_axis_is_established(snapshot, output, id, "height")
    {
        return matches!(prop(child_tag, "pos"), Some("absolute" | "fixed"))
            || parent(snapshot, output, id)
                .and_then(|p| node_opening(output, &p.id))
                .is_some_and(|s| flex_axis(s) == Some("HORIZONTAL"));
    }
    if matches!(prop(child_tag, "pos"), Some("absolute" | "fixed")) {
        return false;
    }
    let Some(parent) = parent(snapshot, output, id) else {
        return false;
    };
    let Some(opening) = node_opening(output, &parent.id) else {
        return false;
    };
    let Some(axis) = flex_axis(opening) else {
        return false;
    };
    if parent.typed_view().string("layoutMode") != Some(axis) {
        return false;
    }
    if field == "layoutGrow" {
        return view
            .number(field)
            .is_some_and(|grow| grow > 0.0 && source == format!("flex=\"{grow}\""));
    }
    view.string(field) == Some("FILL")
        && axis == "VERTICAL"
        && source == "flex=\"1\""
        && (!crate::codegen::vertical_fill_needs_minimum_reset(snapshot, node)
            || prop(child_tag, "minH") == Some("0"))
        && fill_axis_is_established(snapshot, output, id, "height")
}

pub(crate) fn account_for_sizing(snapshot: &Snapshot, output: &mut CodegenOutput) {
    let mut entries = Vec::new();
    let mut diagnostics = Vec::new();
    let nodes: Vec<_> = output
        .source_map
        .entries
        .iter()
        .filter(|e| e.property.is_none())
        .cloned()
        .collect();
    for entry in nodes {
        let Some(id) = entry.node_id.as_deref() else {
            continue;
        };
        let Some(node) = snapshot.nodes.get(id) else {
            continue;
        };
        let Some(opening) = node_opening(output, id) else {
            continue;
        };
        let Some(range) = entry.generated_range.as_ref() else {
            continue;
        };
        for field in ["layoutSizingVertical", "layoutGrow"] {
            if !layout_field_is_semantic(snapshot, node, field) {
                continue;
            }
            for name in ["h", "boxSize", "flex"] {
                let needle = format!("{name}=\"");
                let Some(start) = find_prop(opening, &needle) else {
                    continue;
                };
                let Some(end) = opening[start + needle.len()..].find('"') else {
                    continue;
                };
                let end = start + needle.len() + end + 1;
                if sizing_mapping_matches(snapshot, output, id, field, &opening[start..end]) {
                    entries.push(generated_entry(
                        range.start + start,
                        range.start + end,
                        id,
                        field,
                        None,
                        None,
                        "verified-layout-sizing",
                    ));
                }
            }
        }
        for field in ["width", "height", "layoutSizingVertical"] {
            if field != "layoutSizingVertical"
                && !empty_layout_leaf(output, id)
                && !projects_as_asset(snapshot, node)
            {
                continue;
            }
            if implicit_css_verification(snapshot, output, id, field)["state"] != "accounted-for" {
                continue;
            }
            entries.push(generated_entry(
                range.start,
                range.start + opening.len(),
                id,
                field,
                None,
                None,
                "accounted-for-implicit-flex-stretch",
            ));
            diagnostics.push(devup_mcp_figma::Diagnostic {
                code:"DEVUP_CODEGEN_LAYOUT_ACCOUNTED_FOR".into(),
                node_id:Some(id.into()),property:Some(field.into()),
                message:"Verified implicit flex stretch accounts for this FILL dimension.".into(),
                fidelity_impact:Some(FidelityImpact::None),
                details:Some(json!({"classification":"accounted-for","originalValue":node.typed_view().value(field),
                    "implicitCssVerification":implicit_css_verification(snapshot,output,id,field)})),
                ..Default::default()
            });
        }
    }
    output.source_map.entries.extend(entries);
    output.diagnostics.extend(diagnostics);
}
