use devup_mcp_devup_ui::codegen::{CodegenOptions, CodegenOutput, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::{Value, json};

fn modal() -> Snapshot {
    serde_json::from_str(include_str!("../../../fixtures/r8/modal-snapshot.json")).unwrap()
}

fn asset() -> Snapshot {
    serde_json::from_str(include_str!("../../../fixtures/r14/asset-snapshot.json")).unwrap()
}

fn generate(s: &Snapshot, root: &str) -> CodegenOutput {
    generate_component(
        s,
        root,
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap()
}

fn details(o: &CodegenOutput, id: &str) -> Value {
    o.diagnostics
        .iter()
        .find(|d| {
            d.node_id.as_deref() == Some(id)
                && matches!(
                    d.code.as_str(),
                    "DEVUP_CODEGEN_ABSOLUTE_FALLBACK" | "DEVUP_CODEGEN_ABSOLUTE_VERIFIED"
                )
        })
        .unwrap()
        .details
        .clone()
        .unwrap()
}

#[test]
fn r14_fixed_width_percentage_proves_equal_containing_block_without_changing_css() {
    let o = generate(&modal(), "3997:46582");
    let d = details(&o, "3997:46621");
    assert_eq!(d["components"]["width"]["generatedValue"], "100%");
    assert_eq!(d["components"]["width"]["state"], "verified");
    assert_eq!(
        d["appliedValue"]["widthPreservation"]["resolvedPixels"],
        360.0
    );
    assert_eq!(
        d["appliedValue"]["widthPreservation"]["parentGeneratedValue"],
        "360px"
    );
    assert_eq!(d["classification"], "verified-absolute-components");
    assert_eq!(d["componentSummary"]["unresolved"], json!([]));
}

#[test]
fn r14_percentage_relation_is_independent_of_the_observed_360px_width() {
    for (width, parent_css) in [
        (280.0, "280px"),
        (375.0, "375px"),
        (411.25, "411.25px"),
        (768.0, "768px"),
        (1280.0, "1280px"),
    ] {
        // A minimal empty absolute frame avoids changing the real modal's
        // unrelated child overflow/derived-layout policy at smaller widths.
        let s: Snapshot = serde_json::from_value(json!({"fileKey":"r14-widths","diagnostics":[],"roots":["p"],"nodes":{
            "p":{"id":"p","type":"FRAME","fields":{"width":width,"height":100,"layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED","childrenIds":["c"]}},
            "c":{"id":"c","type":"FRAME","fields":{"parentId":"p","width":width,"height":50,"x":0,"y":0,"layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED","layoutPositioning":"ABSOLUTE","constraints":{"horizontal":"MIN","vertical":"MIN"}}}
        }})).unwrap();
        let d = details(&generate(&s, "p"), "c");
        assert_eq!(d["components"]["width"]["generatedValue"], "100%");
        assert_eq!(d["components"]["width"]["state"], "verified", "{width}");
        assert_eq!(
            d["appliedValue"]["widthPreservation"]["parentGeneratedValue"],
            parent_css
        );
        assert_eq!(
            d["appliedValue"]["widthPreservation"]["resolvedPixels"],
            width
        );
    }
}

#[test]
fn r14_percentage_rejects_mismatch_unknown_parent_and_read_errors() {
    for case in [
        "mismatch",
        "parent-fill",
        "parent-error",
        "child-error",
        "child-fill",
    ] {
        let mut s = modal();
        match case {
            "mismatch" => {
                s.nodes
                    .get_mut("3997:46621")
                    .unwrap()
                    .fields
                    .insert("width".into(), json!(400));
            }
            "parent-fill" => {
                s.nodes
                    .get_mut("3997:46582")
                    .unwrap()
                    .fields
                    .insert("layoutSizingHorizontal".into(), json!("FILL"));
            }
            "parent-error" => {
                s.nodes
                    .get_mut("3997:46582")
                    .unwrap()
                    .field_errors
                    .insert("width".into(), "unavailable".into());
            }
            "child-error" => {
                s.nodes
                    .get_mut("3997:46621")
                    .unwrap()
                    .field_errors
                    .insert("width".into(), "unavailable".into());
            }
            _ => {
                s.nodes
                    .get_mut("3997:46621")
                    .unwrap()
                    .fields
                    .insert("layoutSizingHorizontal".into(), json!("FILL"));
            }
        }
        let o = generate(&s, "3997:46582");
        let d = details(&o, "3997:46621");
        assert_eq!(d["components"]["width"]["state"], "approximated", "{case}");
        assert!(
            d["appliedValue"]["widthPreservation"]["reason"]
                .as_str()
                .is_some_and(|s| !s.is_empty()),
            "{case}"
        );
    }
}

#[test]
fn r14_asset_boundary_rounding_and_inherited_constraints_are_separate() {
    let o = generate(&asset(), "3997:46667");
    let d = details(&o, "3997:46668");
    assert_eq!(d["components"]["width"]["generatedValue"], "360px");
    assert_eq!(d["components"]["height"]["generatedValue"], "117.67px");
    assert_eq!(
        d["components"]["width"]["boundary"]["selectedField"],
        "absoluteRenderBounds"
    );
    assert_eq!(d["components"]["width"]["boundary"]["selectedValue"], 360.0);
    assert_eq!(
        d["components"]["width"]["rounding"]["absoluteErrorPixels"],
        0.0
    );
    assert_eq!(
        d["components"]["height"]["rounding"]["tolerancePixels"],
        0.005
    );
    assert_eq!(d["components"]["height"]["rounding"]["state"], "verified");
    assert_eq!(d["components"]["height"]["state"], "verified");
    assert_eq!(d["components"]["width"]["state"], "verified");
    for (axis, constraint, property, value) in [
        ("horizontal", "SCALE", "left", "0px"),
        ("vertical", "MAX", "bottom", "28.33px"),
    ] {
        let a = &d["components"][axis];
        assert_eq!(a["state"], "approximated");
        assert_eq!(a["constraint"], constraint);
        assert_eq!(a["constraintDeclared"], false);
        assert_eq!(a["constraintInterpretation"]["sourceNodeId"], "3997:46669");
        assert_eq!(a["constraintInterpretation"]["effectiveValue"], constraint);
        assert_eq!(a["constraintInterpretation"]["state"], "assumed");
        assert_eq!(a["generatedProperty"], property);
        assert_eq!(a["generatedValue"], value);
        assert_eq!(a["rounding"]["state"], "verified");
    }
    assert_eq!(
        d["componentSummary"]["verified"],
        json!(["height", "width"])
    );
    assert_eq!(
        d["componentSummary"]["unresolved"],
        json!(["horizontal", "vertical"])
    );
    assert_eq!(
        d["classification"],
        "partially-verified-absolute-components"
    );
}

#[test]
fn r14_asset_boundary_read_error_cannot_be_verified() {
    let mut s = asset();
    s.nodes
        .get_mut("3997:46668")
        .unwrap()
        .field_errors
        .insert("absoluteRenderBounds".into(), "unavailable".into());
    let d = details(&generate(&s, "3997:46667"), "3997:46668");
    assert_eq!(d["components"]["width"]["state"], "approximated");
    assert_eq!(d["components"]["width"]["boundary"]["state"], "unverified");
}

#[test]
fn r14_null_and_absent_constraints_report_actual_fallback_without_promoting_it() {
    for null in [false, true] {
        let mut s = asset();
        if null {
            s.nodes
                .get_mut("3997:46668")
                .unwrap()
                .fields
                .insert("constraints".into(), Value::Null);
        }
        s.nodes
            .get_mut("3997:46669")
            .unwrap()
            .fields
            .remove("constraints");
        let d = details(&generate(&s, "3997:46667"), "3997:46668");
        for axis in ["horizontal", "vertical"] {
            assert_eq!(
                d["components"][axis]["constraintInterpretation"]["origin"],
                "default"
            );
            assert_eq!(
                d["components"][axis]["constraintInterpretation"]["effectiveValue"],
                "MIN"
            );
            assert_eq!(d["components"][axis]["state"], "approximated");
        }
    }
}

#[test]
fn r14_asset_fill_is_not_a_verified_fixed_export_dimension() {
    let mut s = asset();
    s.nodes
        .get_mut("3997:46668")
        .unwrap()
        .fields
        .insert("layoutSizingHorizontal".into(), json!("FILL"));
    let d = details(&generate(&s, "3997:46667"), "3997:46668");
    assert_eq!(d["components"]["width"]["state"], "approximated");
    assert_eq!(
        d["components"]["width"]["boundary"]["selectedField"],
        "absoluteRenderBounds"
    );
}

#[test]
fn r14_assets_without_export_bounds_retain_the_existing_explicit_dimension_proof() {
    let mut s = asset();
    let fields = &mut s.nodes.get_mut("3997:46668").unwrap().fields;
    fields.remove("absoluteRenderBounds");
    fields.insert("width".into(), json!(100));
    fields.insert("height".into(), json!(50));
    let d = details(&generate(&s, "3997:46667"), "3997:46668");
    for (axis, value) in [("width", "100px"), ("height", "50px")] {
        assert_eq!(d["components"][axis]["generatedValue"], value);
        assert_eq!(d["components"][axis]["state"], "preserved");
    }
}

#[test]
fn r14_far_edge_boundary_proof_rejects_parent_size_read_errors() {
    let mut s = asset();
    s.nodes
        .get_mut("3997:46667")
        .unwrap()
        .field_errors
        .insert("height".into(), "unavailable".into());
    let d = details(&generate(&s, "3997:46667"), "3997:46668");
    let vertical = &d["components"]["vertical"];
    assert_eq!(vertical["state"], "approximated");
    assert_eq!(vertical["boundary"]["state"], "unverified");
    assert!(
        vertical["boundary"]["sourceFields"]
            .as_array()
            .unwrap()
            .contains(&json!("parent.height"))
    );
    // Rounding a reported number does not establish that its input is reliable.
    assert_eq!(d["components"]["height"]["boundary"]["state"], "verified");
}

#[test]
fn r16_resolution_explains_raw_mapping_and_verified_width_as_separate_axes() {
    let o = generate(&modal(), "3997:46582");
    let map = serde_json::to_value(&o.source_map).unwrap();
    let entry = map["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["nodeId"] == "3997:46621" && e["property"] == "width")
        .unwrap();
    assert_eq!(entry["resolution"], "raw-fallback");
    assert_eq!(
        details(&o, "3997:46621")["components"]["width"]["state"],
        "verified"
    );
    assert_eq!(map["resolutionSemantics"]["axis"], "mapping-method");
    assert_eq!(
        map["resolutionSemantics"]["verificationAxis"],
        "diagnostics.details.components.*.state"
    );
    assert!(
        map["resolutionSemantics"]["values"]["raw-fallback"]
            .as_str()
            .unwrap()
            .contains("verified")
    );
}

#[test]
fn r16_verified_width_has_no_blocker_and_preserves_width_evidence_alias() {
    let o = generate(&modal(), "3997:46582");
    let d = details(&o, "3997:46621");
    assert!(d["components"]["width"].get("blockedBy").is_some());
    assert_eq!(d["components"]["width"]["blockedBy"], Value::Null);
    assert_eq!(
        d["components"]["width"],
        d["appliedValue"]["widthPreservation"]
    );
}

#[test]
fn r16_embedded_width_reports_unknown_parent_without_promoting_it() {
    let s = modal();
    let o = generate_component(
        &s,
        "3997:46582",
        &CodegenOptions {
            inline_instances: true,
            root_layout: devup_mcp_devup_ui::codegen::RootLayout::Embedded,
            ..Default::default()
        },
    )
    .unwrap();
    let d = details(&o, "3997:46621");
    assert_eq!(d["components"]["width"]["state"], "approximated");
    assert_eq!(
        d["components"]["width"]["blockedBy"],
        "parent-width-unknown"
    );
    assert_eq!(
        d["components"]["width"],
        d["appliedValue"]["widthPreservation"]
    );
}
