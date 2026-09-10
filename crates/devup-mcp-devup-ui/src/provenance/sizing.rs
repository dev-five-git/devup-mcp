use super::*;
use serde_json::Value;

pub(super) fn non_rendering_asset_accounted(
    snapshot: &Snapshot,
    output: &CodegenOutput,
    id: &str,
) -> bool {
    snapshot.nodes.get(id).is_some_and(|node| {
        !node.field_errors.contains_key("absoluteRenderBounds")
            && node
                .typed_view()
                .value("absoluteRenderBounds")
                .is_some_and(Value::is_null)
            && node_opening(output, id).is_some_and(|tag| {
                prop(tag, "visibility") == Some("hidden")
                    && ["src", "bgImage", "maskImage"]
                        .iter()
                        .all(|name| prop(tag, name).is_none())
            })
    })
}

/// Verify emitted props against collected geometry, never against a resolution label.
/// This is a bounded constraint proof, not a browser measurement.
pub(crate) fn absolute_component_verification(
    snapshot: &Snapshot,
    output: &CodegenOutput,
    id: &str,
) -> Value {
    let Some(node) = snapshot.nodes.get(id) else {
        return Value::Null;
    };
    let view = node.typed_view();
    let tag = node_opening(output, id).unwrap_or("");
    let owner = parent(snapshot, output, id);
    let parent_tag = owner
        .and_then(|p| node_opening(output, &p.id))
        .unwrap_or("");
    let no_parent_border = [
        "border",
        "borderWidth",
        "borderLeft",
        "borderRight",
        "borderTop",
        "borderBottom",
        "borderLeftWidth",
        "borderRightWidth",
        "borderTopWidth",
        "borderBottomWidth",
    ]
    .iter()
    .all(|name| prop(parent_tag, name).is_none());
    let dimension = |axis: &str, name: &str, sizing: &str| {
        let original = view.number(axis);
        let generated = prop(tag, name).or_else(|| prop(tag, "boxSize"));
        let preserved = !node.field_errors.contains_key(axis)
            && !node.field_errors.contains_key(sizing)
            && view.string(sizing) == Some("FIXED")
            && original
                .zip(generated.and_then(|v| v.strip_suffix("px")?.parse::<f64>().ok()))
                .is_some_and(|(a, b)| a.is_finite() && a == b);
        let auto_layout = !node.field_errors.contains_key("layoutMode")
            && flex_axis(tag).is_some_and(|mode| view.string("layoutMode") == Some(mode));
        let intrinsic = auto_layout
            && generated.is_none()
            && view.string(sizing) == Some("HUG")
            && !node.field_errors.contains_key(sizing)
            && ["minW", "maxW", "minH", "maxH", "flex"]
                .iter()
                .all(|name| prop(tag, name).is_none());
        // Only source-matching auto-layout containers have this percentage proof.
        let percentage = auto_layout
            && no_parent_border
            && generated == Some("100%")
            && view.string(sizing) == Some("FIXED")
            && !node.field_errors.contains_key(axis)
            && !node.field_errors.contains_key(sizing)
            && ["minW", "maxW", "minH", "maxH"]
                .iter()
                .all(|name| prop(tag, name).is_none())
            && owner.is_some_and(|p| {
                let pv = p.typed_view();
                !p.field_errors.contains_key(axis)
                    && !p.field_errors.contains_key(sizing)
                    && pv.string(sizing) == Some("FIXED")
                    && original
                        .zip(pv.number(axis))
                        .is_some_and(|(a, b)| a.is_finite() && a == b)
                    && original
                        .zip(
                            prop(parent_tag, name)
                                .or_else(|| prop(parent_tag, "boxSize"))
                                .and_then(|v| v.strip_suffix("px")?.parse::<f64>().ok()),
                        )
                        .is_some_and(|(a, b)| a == b)
                    && [
                        "minW", "maxW", "minH", "maxH", "p", "px", "py", "pl", "pr", "pt", "pb",
                    ]
                    .iter()
                    .all(|name| prop(parent_tag, name).is_none())
            });
        json!({"state":if preserved {"preserved"} else if intrinsic || percentage {"verified"} else {"approximated"},"fidelityImpact":if preserved || intrinsic || percentage {"none"} else {"approximated"},
            "sourceSizing":view.string(sizing),"sourceValue":original,"generatedValue":generated,
            "reason":if preserved {"Explicit generated pixels equal the collected FIXED dimension."} else if intrinsic {"Source HUG is emitted as intrinsic size on a matching auto-layout container without dimension overrides; this verifies sizing intent, not measured pixels."} else if percentage {"Auto-layout percentage equals the source FIXED dimension in an explicit equal-sized FIXED parent without padding or borders."} else {"No verified explicit FIXED dimension; percentage, intrinsic or missing sizes need a separate sizing proof."},
            "resolutionCondition":"Verify the emitted dimension against source sizing; percentage sizing also requires a proven containing-block size and responsive relation."})
    };
    let common = prop(tag, "pos") == Some("absolute")
        && owner.is_some_and(|p| p.node_type == "FRAME")
        && matches!(
            prop(parent_tag, "pos"),
            Some("relative" | "absolute" | "fixed")
        )
        && [node].into_iter().chain(owner).all(|n| {
            let v = n.typed_view();
            !["x", "y", "width", "height", "rotation", "constraints"]
                .iter()
                .any(|f| n.field_errors.contains_key(*f))
                && v.number("rotation").is_none_or(|r| r == 0.0)
        })
        && no_parent_border
        && prop(parent_tag, "transform").is_none()
        && ["m", "mx", "my", "ml", "mr", "mt", "mb", "inset"]
            .iter()
            .all(|p| prop(tag, p).is_none());
    let horizontal = view
        .value("constraints")
        .and_then(|v| v.get("horizontal"))
        .and_then(Value::as_str)
        .or_else(|| view.value("constraints").is_none().then_some("MIN"));
    let vertical = view
        .value("constraints")
        .and_then(|v| v.get("vertical"))
        .and_then(Value::as_str)
        .or_else(|| view.value("constraints").is_none().then_some("MIN"));
    let expected_transform = match (horizontal, vertical) {
        (Some("CENTER"), Some("CENTER")) => Some("translate(-50%, -50%)"),
        (Some("CENTER"), _) => Some("translateX(-50%)"),
        (_, Some("CENTER")) => Some("translateY(-50%)"),
        _ => None,
    };
    let width = dimension("width", "w", "layoutSizingHorizontal");
    let height = dimension("height", "h", "layoutSizingVertical");
    let axis = |constraint: Option<&str>,
                coordinate: &str,
                size: &str,
                css: &str,
                opposite: &str| {
        let offset = view.number(coordinate);
        let own_size = view.number(size);
        let parent_size = owner.and_then(|p| p.typed_view().number(size));
        let (css, opposite) = if constraint == Some("MAX") {
            (opposite, css)
        } else {
            (css, opposite)
        };
        let generated = prop(tag, css);
        let verified = common
            && prop(tag, opposite).is_none()
            && prop(tag, "transform") == expected_transform
            && match constraint {
                Some("CENTER") => {
                    generated == Some("50%")
                        && offset
                            .zip(own_size)
                            .zip(parent_size)
                            .is_some_and(|((x, w), p)| {
                                x.is_finite()
                                    && w.is_finite()
                                    && p.is_finite()
                                    && (x + w / 2.0 - p / 2.0).abs() < 1e-6
                            })
                }
                Some("MIN") => offset
                    .zip(generated.and_then(|v| v.strip_suffix("px")?.parse::<f64>().ok()))
                    .is_some_and(|(a, b)| a.is_finite() && a == b),
                Some("MAX") => {
                    (if size == "width" { &width } else { &height })["fidelityImpact"] == "none"
                        && offset
                            .zip(own_size)
                            .zip(parent_size)
                            .zip(generated.and_then(|v| v.strip_suffix("px")?.parse::<f64>().ok()))
                            .is_some_and(|(((x, w), p), margin)| {
                                x.is_finite()
                                    && w.is_finite()
                                    && p.is_finite()
                                    && (p - x - w - margin).abs() < 1e-6
                            })
                }
                _ => false,
            };
        json!({"state":if verified {"verified"} else {"approximated"},"fidelityImpact":if verified {"none"} else {"approximated"},
            "constraint":constraint,"constraintDeclared":view.value("constraints").is_some(),"sourceOffset":offset,"sourceSize":own_size,"parentSize":parent_size,
            "generatedProperty":css,"generatedValue":generated,"transform":prop(tag,"transform"),
            "reason":if verified {"Collected offset and emitted CSS agree in the emitted immediate positioned parent; without a declared constraint only the captured local offset is verified."} else {"Constraint, geometry, read errors, containing block or emitted props do not satisfy the bounded CENTER/MIN/MAX proof; STRETCH/SCALE and MAX with unverified sizing require separate proofs."},
            "resolutionCondition":"Collect error-free local geometry and constraints; establish the emitted immediate containing block; verify MIN offset, zero CENTER offset with exact translation, or MAX margin with verified sizing, without conflicting props."})
    };
    json!({"height":height,"width":width,
        "horizontal":axis(horizontal,"x","width","left","right"),"vertical":axis(vertical,"y","height","top","bottom"),
        "containingBlock":{"parentId":owner.map(|p|&p.id),"generatedParent":parent_tag,"generatedChild":tag}})
}

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
        for field in ["width", "height"] {
            if !content_sizing_accounted(snapshot, output, id, field) {
                continue;
            }
            entries.push(generated_entry(
                range.start,
                range.start + opening.len(),
                id,
                field,
                None,
                None,
                "accounted-for-content-sizing",
            ));
            diagnostics.push(devup_mcp_figma::Diagnostic {
                code: "DEVUP_CODEGEN_LAYOUT_ACCOUNTED_FOR".into(),
                node_id: Some(id.into()), property: Some(field.into()),
                message: "Source text auto-resize is represented by intentionally omitted fixed CSS sizing; pixel equivalence remains unmeasured.".into(),
                severity: Some(devup_mcp_figma::DiagnosticSeverity::Info),
                fidelity_impact: Some(FidelityImpact::None),
                details: Some(json!({"classification":"text-auto-size", "resolution":"accounted-for-content-sizing",
                    "originalValue":node.typed_view().value(field), "textAutoResize":node.typed_view().string("textAutoResize"),
                    "appliedValue":{"state":"emitted", "generatedNodeId":id,"generatedSource":opening,"propertyMappingVerified":true},
                    "reason":"The collected textAutoResize mode directs content sizing on this axis and the generated Text intentionally omits its fixed dimension.",
                    "verification":{"state":"unverified","reasonCode":"font-metrics-not-measured",
                        "reason":"Browser font metrics and pixel dimensions have not been measured; content-sizing semantics are accounted for, not pixel equivalence."}})),
                ..Default::default()
            });
        }
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

/// Mirror the renderer's intentional text-size omission and recheck the actual
/// element. This accounts for a sizing instruction, never its measured pixels.
pub(super) fn content_sizing_accounted(
    snapshot: &Snapshot,
    output: &CodegenOutput,
    id: &str,
    field: &str,
) -> bool {
    let Some(node) = snapshot.nodes.get(id) else {
        return false;
    };
    let view = node.typed_view();
    let Some(tag) = node_opening(output, id) else {
        return false;
    };
    let mode = view.string("textAutoResize");
    node.node_type == "TEXT"
        && tag
            .trim_start()
            .strip_prefix("<Text")
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
        && matches!(field, "width" | "height")
        && [
            field,
            "textAutoResize",
            "layoutSizingHorizontal",
            "layoutSizingVertical",
            "characters",
        ]
        .iter()
        .all(|f| !node.field_errors.contains_key(*f))
        && view.string("layoutSizingHorizontal") == Some("FIXED")
        && view.string("layoutSizingVertical") == Some("FIXED")
        && view.string("characters").is_some()
        && view.number(field).is_some_and(f64::is_finite)
        && (mode == Some("WIDTH_AND_HEIGHT")
            || (field == "height"
                && mode == Some("HEIGHT")
                && !node.field_errors.contains_key("width")
                && view.number("width").is_some_and(|w| {
                    prop(tag, "w").and_then(|v| v.strip_suffix("px")?.parse::<f64>().ok())
                        == Some(w)
                })))
        && [
            if field == "width" { "w" } else { "h" },
            "boxSize",
            "width",
            "height",
            "style",
            "css",
            "as",
        ]
        .iter()
        .all(|name| find_prop(tag, &format!("{name}=")).is_none())
}
