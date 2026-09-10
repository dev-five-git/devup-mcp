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

fn css_px(value: &str) -> Option<f64> {
    let n = value
        .strip_suffix("px")
        .unwrap_or(value)
        .parse::<f64>()
        .ok()?;
    n.is_finite().then_some(n)
}

fn horizontal_allocation(
    snapshot: &Snapshot,
    output: &CodegenOutput,
    id: &str,
    container: &devup_mcp_figma::RawNode,
    opening: &str,
) -> Value {
    let mut proof = json!({"state":"not-accounted-for", "cssBehavior":"flex main-axis grow allocation",
        "parentId":container.id,"parentLayoutMode":"HORIZONTAL", "childSizing":"FILL",
        "generatedParent":opening,"generatedChild":node_opening(output,id),
        "flexBasis":"0%","flexShrink":1,
        "evidenceLimit":"Verifies the responsive allocation contract at the captured parent/sibling sizes, not browser font metrics or pixel equivalence."});
    let check = || -> Result<Value, &'static str> {
        let pv = container.typed_view();
        if pv.string("layoutMode") != Some("HORIZONTAL") {
            return Err("source-direction-mismatch");
        }
        if prop(opening, "flexWrap").is_some_and(|v| v != "nowrap")
            || pv.string("layoutWrap") == Some("WRAP")
        {
            return Err("wrapped-flex-line");
        }
        if !fill_axis_is_established(snapshot, output, id, "width") {
            return Err("parent-width-unestablished");
        }
        let width = pv
            .number("width")
            .filter(|n| n.is_finite() && *n > 0.0)
            .ok_or("parent-width-missing")?;
        if let Some(w) = prop(opening, "w").or_else(|| prop(opening, "boxSize"))
            && w != "100%"
            && css_px(w).is_none_or(|w| (w - width).abs() > 0.5)
        {
            return Err("parent-width-mismatch");
        }
        if [
            "minW",
            "maxW",
            "border",
            "borderWidth",
            "borderLeft",
            "borderRight",
            "borderLeftWidth",
            "borderRightWidth",
        ]
        .iter()
        .any(|p| prop(opening, p).is_some())
        {
            return Err("parent-box-constraint");
        }
        let mut padding = 0.0;
        for (field, side) in [("paddingLeft", "pl"), ("paddingRight", "pr")] {
            let source = pv.number(field).unwrap_or(0.0);
            let emitted = prop(opening, side)
                .or_else(|| prop(opening, "px"))
                .or_else(|| prop(opening, "p"))
                .map(css_px)
                .unwrap_or(Some(0.0))
                .ok_or("parent-padding-unverified")?;
            if (source - emitted).abs() > 0.5 {
                return Err("parent-padding-mismatch");
            }
            padding += emitted;
        }
        let gap = prop(opening, "columnGap")
            .or_else(|| prop(opening, "gap"))
            .map(css_px)
            .unwrap_or(Some(0.0))
            .ok_or("gap-unverified")?;
        if gap < 0.0 || (gap - pv.number("itemSpacing").unwrap_or(0.0)).abs() > 0.5 {
            return Err("gap-mismatch");
        }
        let mut siblings = Vec::new();
        let mut fixed = 0.0;
        let mut total_grow = 0.0;
        let mut own_grow = 0.0;
        for sid in pv.child_ids() {
            let sibling = snapshot.nodes.get(sid).ok_or("sibling-missing")?;
            let sv = sibling.typed_view();
            if sv.bool("visible") == Some(false)
                || sv.string("layoutPositioning") == Some("ABSOLUTE")
            {
                continue;
            }
            if parent(snapshot, output, sid).is_none_or(|p| p.id != container.id) {
                return Err("sibling-parent-mismatch");
            }
            let tag = node_opening(output, sid).ok_or("sibling-css-missing")?;
            if matches!(prop(tag, "pos"), Some("absolute" | "fixed")) {
                return Err("sibling-position-mismatch");
            }
            if [
                "m",
                "mx",
                "ml",
                "mr",
                "minW",
                "maxW",
                "flexBasis",
                "flexGrow",
            ]
            .iter()
            .any(|p| prop(tag, p).is_some())
            {
                return Err("sibling-flex-constraint");
            }
            if prop(tag, "flexShrink").is_some_and(|v| !matches!(v, "0" | "1")) {
                return Err("sibling-shrink-unverified");
            }
            let measured = sv
                .number("width")
                .filter(|w| w.is_finite() && *w >= 0.0)
                .ok_or("sibling-width-missing")?;
            if sv.string("layoutSizingHorizontal") == Some("FILL") {
                let primitive = tag
                    .trim_start()
                    .strip_prefix('<')
                    .unwrap_or("")
                    .split(|c: char| c.is_whitespace() || c == '/')
                    .next()
                    .unwrap_or("");
                if !matches!(primitive, "Box" | "Flex" | "Center" | "VStack") {
                    return Err("intrinsic-replaced-minimum-unverified");
                }
                let grow = prop(tag, "flex")
                    .and_then(|s| s.parse::<f64>().ok())
                    .filter(|g| g.is_finite() && *g > 0.0)
                    .ok_or("flex-shorthand-unverified")?;
                if prop(tag, "w").is_some()
                    || prop(tag, "boxSize").is_some()
                    || prop(tag, "aspectRatio").is_some()
                    || sv.child_ids().next().is_some()
                    || sibling.node_type == "TEXT"
                    || [
                        "p",
                        "px",
                        "pl",
                        "pr",
                        "border",
                        "borderWidth",
                        "borderLeft",
                        "borderRight",
                        "borderLeftWidth",
                        "borderRightWidth",
                    ]
                    .iter()
                    .any(|p| prop(tag, p).is_some())
                {
                    return Err("flex-minimum-unverified");
                }
                if sv
                    .number("layoutGrow")
                    .is_some_and(|g| (g - grow).abs() > 0.001)
                {
                    return Err("flex-grow-mismatch");
                }
                total_grow += grow;
                if sid == id {
                    own_grow = grow;
                }
                siblings.push(json!({"nodeId":sid,"grow":grow,"basis":"0%","shrink":prop(tag,"flexShrink").unwrap_or("1"),"capturedWidth":measured}));
            } else {
                if prop(tag, "flex").is_some() {
                    return Err("sibling-flex-basis-unverified");
                }
                let explicit = prop(tag, "w")
                    .or_else(|| prop(tag, "boxSize"))
                    .and_then(css_px);
                let intrinsic_text = sibling.node_type == "TEXT"
                    && sv.string("layoutSizingHorizontal") == Some("HUG")
                    && sv.string("characters").is_some_and(|s| !s.contains('\n'))
                    && prop(tag, "flex").is_none()
                    && prop(tag, "w").is_none()
                    && prop(tag, "boxSize").is_none()
                    && (prop(tag, "typography").is_some() || prop(tag, "fontSize").is_some());
                if !intrinsic_text && explicit.is_none_or(|w| (w - measured).abs() > 0.5) {
                    return Err("sibling-basis-unverified");
                }
                fixed += measured;
                siblings.push(json!({"nodeId":sid,"basis":if intrinsic_text {"intrinsic-text"}else{"explicit-width"},"capturedWidth":measured,"shrink":prop(tag,"flexShrink").unwrap_or("1")}));
            }
        }
        if own_grow <= 0.0 {
            return Err("child-grow-missing");
        }
        let free = width - padding - gap * (siblings.len().saturating_sub(1) as f64) - fixed;
        if free < 0.0 {
            return Err("negative-free-space-shrink-unverified");
        }
        // With positive free space, shrink does not participate. Empty flex items
        // have zero automatic minimum; constraints and intrinsic children reject above.
        let divisor = total_grow.max(1.0);
        for sibling in &siblings {
            if let Some(g) = sibling["grow"].as_f64()
                && (free * g / divisor - sibling["capturedWidth"].as_f64().unwrap()).abs() > 0.5
            {
                return Err("allocated-width-mismatch");
            }
        }
        Ok(
            json!({"parentWidth":width,"padding":padding,"gap":gap,"siblings":siblings,"freeSpace":free,
            "allocatedWidth":free*own_grow/divisor,"totalGrow":total_grow,"shrinkActive":false}),
        )
    };
    match check() {
        Ok(allocation) => {
            proof["state"] = json!("accounted-for");
            proof["allocation"] = allocation;
            proof["reasonCode"] = json!("verified-main-axis-allocation");
            proof["reason"] = json!(
                "The emitted flex shorthand, parent width, padding, siblings and gaps preserve positive remaining-space allocation; no minimum or shrink constraint overrides it."
            );
        }
        Err(reason) => {
            proof["reasonCode"] = json!(reason);
            proof["reason"] = json!(format!("Cannot prove main-axis allocation: {reason}."));
        }
    }
    proof
}

fn intrinsic_height(
    snapshot: &Snapshot,
    output: &CodegenOutput,
    id: &str,
    seen: &mut BTreeSet<String>,
) -> bool {
    if !seen.insert(id.into()) {
        return false;
    }
    let Some(n) = snapshot.nodes.get(id) else {
        return false;
    };
    let v = n.typed_view();
    let Some(tag) = node_opening(output, id) else {
        return false;
    };
    if v.bool("visible") == Some(false)
        || v.string("layoutPositioning") == Some("ABSOLUTE")
        || matches!(prop(tag, "pos"), Some("absolute" | "fixed"))
    {
        return false;
    }
    if prop(tag, "h")
        .or_else(|| prop(tag, "boxSize"))
        .and_then(css_px)
        .is_some_and(|h| h > 0.0)
    {
        return true;
    }
    if prop(tag, "h").is_some()
        || prop(tag, "boxSize").is_some()
        || v.string("layoutSizingVertical") != Some("HUG")
    {
        return false;
    }
    if n.node_type == "TEXT" {
        return v.string("characters").is_some_and(|s| !s.is_empty())
            && (prop(tag, "typography").is_some() || prop(tag, "fontSize").is_some());
    }
    flex_axis(tag).is_some()
        && v.child_ids().any(|sid| {
            parent(snapshot, output, sid).is_some() && intrinsic_height(snapshot, output, sid, seen)
        })
}

fn hug_row_has_height(
    snapshot: &Snapshot,
    output: &CodegenOutput,
    container: &devup_mcp_figma::RawNode,
    id: &str,
) -> bool {
    let Some(tag) = node_opening(output, &container.id) else {
        return false;
    };
    container.typed_view().string("layoutSizingVertical") == Some("HUG")
        && prop(tag, "flexWrap").is_none()
        && container.typed_view().string("layoutWrap") != Some("WRAP")
        && container
            .typed_view()
            .child_ids()
            .filter(|sid| *sid != id)
            .any(|sid| {
                parent(snapshot, output, sid).is_some()
                    && intrinsic_height(snapshot, output, sid, &mut BTreeSet::new())
            })
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
    if width && !cross && view.string(sizing) == Some("FILL") {
        return horizontal_allocation(snapshot, output, id, parent, parent_tag);
    }
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
        && (fill_axis_is_established(snapshot, output, id, if width { "width" } else { "height" })
            || (!width
                && field == "layoutSizingVertical"
                && hug_row_has_height(snapshot, output, parent, id)));
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
                if field == "width"
                    && parent(snapshot, output, id)
                        .is_some_and(|p| p.typed_view().string("layoutMode") == Some("HORIZONTAL"))
                {
                    "accounted-for-implicit-flex-grow"
                } else {
                    "accounted-for-implicit-flex-stretch"
                },
            ));
            diagnostics.push(devup_mcp_figma::Diagnostic {
                code:"DEVUP_CODEGEN_LAYOUT_ACCOUNTED_FOR".into(),
                node_id:Some(id.into()),property:Some(field.into()),
                message:"Verified implicit flex sizing accounts for this FILL dimension.".into(),
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
