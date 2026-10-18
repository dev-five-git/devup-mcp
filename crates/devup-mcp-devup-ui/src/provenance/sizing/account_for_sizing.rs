

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
        for field in [
            "layoutSizingHorizontal",
            "layoutSizingVertical",
            "layoutGrow",
        ] {
            if !layout_field_is_semantic(snapshot, node, field) {
                continue;
            }
            for name in ["w", "h", "boxSize", "flex", "alignSelf"] {
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
        for field in [
            "width",
            "height",
            "layoutSizingHorizontal",
            "layoutSizingVertical",
        ] {
            if field == "layoutSizingHorizontal"
                && entries.iter().any(|e| {
                    e.node_id.as_deref() == Some(id) && e.property.as_deref() == Some(field)
                })
            {
                continue;
            }
            if !matches!(field, "layoutSizingHorizontal" | "layoutSizingVertical")
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
                if matches!(field, "width" | "layoutSizingHorizontal")
                    && parent(snapshot, output, id)
                        .is_some_and(|p| p.typed_view().string("layoutMode") == Some("HORIZONTAL"))
                {
                    "accounted-for-implicit-flex-grow"
                } else {
                    "accounted-for-implicit-flex-stretch"
                },
            ));
            // The new horizontal obligation is carried by the verified source-map
            // entry. Avoid duplicating full parent/child excerpts in every response
            // envelope; keep all pre-existing R6/R7 dimension diagnostics intact.
            if field == "layoutSizingHorizontal" {
                continue;
            }
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

#[cfg(test)]
mod r14_tests {
    use super::*;
    use crate::codegen::{CodegenOptions, generate_component};

    #[test]
    fn r14_rounding_rejects_a_centipixel_change_in_generated_height() {
        let s: Snapshot =
            serde_json::from_str(include_str!("../../../../../fixtures/r14/asset-snapshot.json"))
                .unwrap();
        let mut o = generate_component(&s, "3997:46667", &CodegenOptions::default()).unwrap();
        let before = absolute_component_verification(&s, &o, "3997:46668");
        assert_eq!(before["height"]["rounding"]["state"], "verified");
        // Same byte length keeps source ranges valid, exercising the audit of
        // actual emitted values instead of trusting a resolution label.
        o.tsx = o.tsx.replace("117.67px", "117.66px");
        let after = absolute_component_verification(&s, &o, "3997:46668");
        assert_eq!(after["height"]["rounding"]["state"], "unverified");
        assert_eq!(after["height"]["state"], "approximated");
    }

    #[test]
    fn r14_percentage_rechecks_actual_parent_pixels() {
        let s: Snapshot =
            serde_json::from_str(include_str!("../../../../../fixtures/r8/modal-snapshot.json"))
                .unwrap();
        let mut o = generate_component(
            &s,
            "3997:46582",
            &CodegenOptions {
                inline_instances: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            absolute_component_verification(&s, &o, "3997:46621")["width"]["state"],
            "verified"
        );
        o.tsx = o.tsx.replacen("w=\"360px\"", "w=\"359px\"", 1);
        assert_eq!(
            absolute_component_verification(&s, &o, "3997:46621")["width"]["state"],
            "approximated"
        );
    }
}

#[cfg(test)]
mod r16_tests {
    use super::*;
    use crate::codegen::{CodegenOptions, generate_component};

    fn width_case(case: &str, expected: &str) {
        let mut s: Snapshot =
            serde_json::from_str(include_str!("../../../../../fixtures/r8/modal-snapshot.json"))
                .unwrap();
        let mut o = generate_component(
            &s,
            "3997:46582",
            &CodegenOptions {
                inline_instances: true,
                ..Default::default()
            },
        )
        .unwrap();
        match case {
            "unknown" => o.tsx = o.tsx.replacen("w=\"360px\"", "w=\"100% \"", 1),
            "unequal" => o.tsx = o.tsx.replacen("w=\"360px\"", "w=\"359px\"", 1),
            "read" => {
                s.nodes
                    .get_mut("3997:46582")
                    .unwrap()
                    .field_errors
                    .insert("width".into(), "unavailable".into());
            }
            "child-read" => {
                s.nodes
                    .get_mut("3997:46621")
                    .unwrap()
                    .field_errors
                    .insert("width".into(), "unavailable".into());
            }
            "child-sizing" => {
                s.nodes
                    .get_mut("3997:46621")
                    .unwrap()
                    .fields
                    .insert("layoutSizingHorizontal".into(), json!("FILL"));
            }
            "sizing" => {
                s.nodes
                    .get_mut("3997:46582")
                    .unwrap()
                    .fields
                    .insert("layoutSizingHorizontal".into(), json!("FILL"));
            }
            // Equal byte length keeps private provenance ranges valid.
            "conflict" => {
                o.tsx = o
                    .tsx
                    .replacen("overflow=\"hidden\"", "minW=\"360px\"     ", 1)
            }
            "padding" => {
                o.tsx = o
                    .tsx
                    .replacen("overflow=\"hidden\"", "p=\"1px\"          ", 1)
            }
            "containing" => o.tsx = o.tsx.replacen("pos=\"relative\"", "pos=\"static  \"", 1),
            _ => panic!("unknown case"),
        }
        let c = absolute_component_verification(&s, &o, "3997:46621");
        assert_eq!(c["width"]["state"], "approximated");
        assert_eq!(c["width"]["resolvedPixels"], Value::Null);
        assert_eq!(c["width"]["blockedBy"], expected);
    }
    #[test]
    fn r16_parent_width_unknown() {
        width_case("unknown", "parent-width-unknown");
    }
    #[test]
    fn r16_parent_width_unequal() {
        width_case("unequal", "parent-width-unequal");
    }
    #[test]
    fn r16_width_read_error() {
        width_case("read", "read-error");
    }
    #[test]
    fn r16_child_width_read_error() {
        width_case("child-read", "read-error");
    }
    #[test]
    fn r16_child_width_non_fixed_sizing() {
        width_case("child-sizing", "non-fixed-sizing");
    }
    #[test]
    fn r16_width_non_fixed_sizing() {
        width_case("sizing", "non-fixed-sizing");
    }
    #[test]
    fn r16_width_conflicting_dimension_props() {
        width_case("conflict", "conflicting-dimension-props");
    }
    #[test]
    fn r16_width_parent_padding() {
        width_case("padding", "parent-padding-or-border");
    }
    #[test]
    fn r16_width_containing_block() {
        width_case("containing", "containing-block-unproven");
    }

    #[test]
    fn r16_asset_blockers_follow_the_selected_boundary_proof() {
        for (field, expected) in [
            ("absoluteRenderBounds", "read-error"),
            ("layoutSizingHorizontal", "non-fixed-sizing"),
        ] {
            let mut s: Snapshot =
                serde_json::from_str(include_str!("../../../../../fixtures/r14/asset-snapshot.json"))
                    .unwrap();
            let o = generate_component(&s, "3997:46667", &CodegenOptions::default()).unwrap();
            if field == "absoluteRenderBounds" {
                s.nodes
                    .get_mut("3997:46668")
                    .unwrap()
                    .field_errors
                    .insert(field.into(), "unavailable".into());
            } else {
                s.nodes
                    .get_mut("3997:46668")
                    .unwrap()
                    .fields
                    .insert(field.into(), json!("FILL"));
            }
            let c = absolute_component_verification(&s, &o, "3997:46668");
            assert_eq!(c["width"]["state"], "approximated");
            assert_eq!(c["width"]["blockedBy"], expected);
        }
    }
}
