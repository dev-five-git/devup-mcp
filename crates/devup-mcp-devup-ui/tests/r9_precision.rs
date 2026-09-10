use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::{FidelityImpact, Snapshot};
use serde_json::json;

fn modal() -> Snapshot {
    serde_json::from_str(include_str!("../../../fixtures/r8/modal-snapshot.json")).unwrap()
}

#[test]
fn r9_non_rendering_requires_explicit_evidence() {
    for (bounds, expected) in [
        (Some(json!(null)), FidelityImpact::None),
        (None, FidelityImpact::Approximated),
    ] {
        let mut s: Snapshot = serde_json::from_value(json!({"fileKey":"r9", "diagnostics":[], "roots":["a"], "nodes":{"a":{"id":"a","type":"RECTANGLE","fields":{"visible":true,"isAsset":true,"width":24,"height":24,"absoluteBoundingBox":{"x":0,"y":0,"width":24,"height":24},"fills":[{"type":"IMAGE","imageHash":"r9","visible":true}]}}}})).unwrap();
        if let Some(bounds) = bounds {
            s.nodes
                .get_mut("a")
                .unwrap()
                .fields
                .insert("absoluteRenderBounds".into(), bounds);
        }
        let o = generate_component(&s, "a", &CodegenOptions::default()).unwrap();
        let d = o
            .diagnostics
            .iter()
            .find(|d| d.code == "DEVUP_CODEGEN_NON_RENDERING_ASSET")
            .unwrap();
        assert_eq!(d.fidelity_impact, Some(expected));
        let details = d.details.as_ref().unwrap();
        assert_eq!(
            details["verification"]["state"],
            if expected == FidelityImpact::None {
                "accounted-for"
            } else {
                "unverified"
            }
        );
        assert_eq!(details["verification"]["field"], "absoluteRenderBounds");
    }
}

#[test]
fn r9_absolute_reports_verified_axes_and_remaining_width() {
    for constraint in ["CENTER", "SCALE", "STRETCH", "MAX"] {
        let mut s = modal();
        s.nodes.get_mut("3997:46621").unwrap().fields.insert(
            "constraints".into(),
            json!({"horizontal":constraint,"vertical":"MIN"}),
        );
        let o = generate_component(
            &s,
            "3997:46582",
            &CodegenOptions {
                inline_instances: true,
                ..Default::default()
            },
        )
        .unwrap();
        let d = o
            .diagnostics
            .iter()
            .find(|d| {
                d.code == "DEVUP_CODEGEN_ABSOLUTE_FALLBACK"
                    && d.node_id.as_deref() == Some("3997:46621")
            })
            .unwrap();
        let details = d.details.as_ref().unwrap();
        assert_eq!(details["components"]["height"]["state"], "preserved");
        assert_eq!(
            details["components"]["horizontal"]["state"],
            if constraint == "CENTER" {
                "verified"
            } else {
                "approximated"
            }
        );
        assert_eq!(details["components"]["vertical"]["state"], "verified");
        assert_eq!(details["components"]["width"]["state"], "approximated");
        assert!(details["resolutionConditions"].is_array());
    }
}

#[test]
fn r9_preserved_height_has_verified_resolution() {
    let o = generate_component(
        &modal(),
        "3997:46582",
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    let e = o
        .source_map
        .entries
        .iter()
        .find(|e| {
            e.node_id.as_deref() == Some("3997:46621") && e.property.as_deref() == Some("height")
        })
        .unwrap();
    assert_eq!(e.generated_property.as_deref(), Some("h=\"740px\""));
    assert_eq!(e.resolution, "verified-explicit-dimension");
}

#[test]
fn r9_source_map_documents_optional_identifiers_and_resolutions() {
    let docs = include_str!("../../../README.md");
    for text in [
        "variableId",
        "assetId",
        "styleId",
        "verified-explicit-dimension",
        "raw-fallback",
        "verified-layout-sizing",
    ] {
        assert!(docs.contains(text), "missing {text}");
    }
}

#[test]
fn r9_center_offset_and_field_errors_are_not_verified() {
    for field in ["x", "constraints"] {
        let mut s = modal();
        let n = s.nodes.get_mut("3997:46621").unwrap();
        if field == "x" {
            n.fields.insert("x".into(), json!(20));
        } else {
            n.field_errors
                .insert("constraints".into(), "unavailable".into());
        }
        let o = generate_component(
            &s,
            "3997:46582",
            &CodegenOptions {
                inline_instances: true,
                ..Default::default()
            },
        )
        .unwrap();
        let d = o
            .diagnostics
            .iter()
            .find(|d| {
                d.code == "DEVUP_CODEGEN_ABSOLUTE_FALLBACK"
                    && d.node_id.as_deref() == Some("3997:46621")
            })
            .unwrap();
        assert_eq!(
            d.details.as_ref().unwrap()["components"]["horizontal"]["state"],
            "approximated"
        );
        assert_eq!(d.fidelity_impact, Some(FidelityImpact::Approximated));
    }
}

#[test]
fn r9_hidden_node_records_accounted_for_evidence() {
    let mut s = modal();
    s.nodes
        .get_mut("3997:46621")
        .unwrap()
        .fields
        .insert("visible".into(), json!(false));
    let o = generate_component(
        &s,
        "3997:46582",
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    let d = o
        .diagnostics
        .iter()
        .find(|d| {
            d.node_id.as_deref() == Some("3997:46621")
                && d.code == "DEVUP_CODEGEN_NON_RENDERING_ASSET"
        })
        .unwrap();
    assert_eq!(d.fidelity_impact, Some(FidelityImpact::None));
    assert_eq!(
        d.details.as_ref().unwrap()["verification"]["field"],
        "visible"
    );
}

#[test]
fn r9_captured_non_rendering_asset_and_read_error_are_distinguished() {
    for error in [false, true] {
        let mut s: Snapshot =
            serde_json::from_str(include_str!("../../../fixtures/r9/non-rendering-node.json"))
                .unwrap();
        // The saved diagnostic printed null, but the raw capture omitted the key.
        assert!(
            !s.nodes["3997:46703"]
                .fields
                .contains_key("absoluteRenderBounds")
        );
        let unknown = generate_component(&s, "3997:46703", &CodegenOptions::default()).unwrap();
        assert_eq!(
            unknown
                .diagnostics
                .iter()
                .find(|d| d.code == "DEVUP_CODEGEN_NON_RENDERING_ASSET")
                .unwrap()
                .fidelity_impact,
            Some(FidelityImpact::Approximated)
        );
        // Model an explicit successful collection separately from that old capture.
        s.nodes
            .get_mut("3997:46703")
            .unwrap()
            .fields
            .insert("absoluteRenderBounds".into(), json!(null));
        if error {
            s.nodes
                .get_mut("3997:46703")
                .unwrap()
                .field_errors
                .insert("absoluteRenderBounds".into(), "read failed".into());
        }
        let o = generate_component(&s, "3997:46703", &CodegenOptions::default()).unwrap();
        let d = o
            .diagnostics
            .iter()
            .find(|d| d.code == "DEVUP_CODEGEN_NON_RENDERING_ASSET")
            .unwrap();
        assert_eq!(
            d.fidelity_impact,
            Some(if error {
                FidelityImpact::Approximated
            } else {
                FidelityImpact::None
            })
        );
        assert!(!o.tsx.contains("src="));
        assert_eq!(
            d.details.as_ref().unwrap()["verification"]["state"],
            if error { "unverified" } else { "accounted-for" }
        );
    }
}

#[test]
fn r9_verified_absolute_components_clear_only_their_own_impact() {
    let mut s = modal();
    let n = s.nodes.get_mut("3997:46621").unwrap();
    n.fields.insert("width".into(), json!(300));
    n.fields.insert("x".into(), json!(30));
    n.fields.insert("childrenIds".into(), json!([]));
    let o = generate_component(
        &s,
        "3997:46582",
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    let d = o
        .diagnostics
        .iter()
        .find(|d| {
            d.node_id.as_deref() == Some("3997:46621")
                && d.property.as_deref() == Some("layoutPositioning")
        })
        .unwrap();
    assert_eq!(d.fidelity_impact, Some(FidelityImpact::None));
    assert_eq!(
        d.details.as_ref().unwrap()["components"]["width"]["state"],
        "preserved"
    );
}

#[test]
fn r9_hidden_absolute_root_has_no_placement_fidelity_loss() {
    let mut s = modal();
    s.nodes
        .get_mut("3997:46621")
        .unwrap()
        .fields
        .insert("visible".into(), json!(false));
    let o = generate_component(
        &s,
        "3997:46621",
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        !o.diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_ABSOLUTE_FALLBACK")
    );
    assert_eq!(o.fidelity_report.impacts.approximated, 0);
}

#[test]
fn r9_non_rendering_asset_coverage_rechecks_raw_and_generated_evidence() {
    let mut s: Snapshot =
        serde_json::from_str(include_str!("../../../fixtures/r9/non-rendering-node.json")).unwrap();
    s.nodes
        .get_mut("3997:46703")
        .unwrap()
        .fields
        .insert("absoluteRenderBounds".into(), json!(null));
    let mut o = generate_component(&s, "3997:46703", &CodegenOptions::default()).unwrap();
    assert!(o.fidelity_report.assets.complete());
    o.tsx = o
        .tsx
        .replace("visibility=\"hidden\"", "visibility=\"unset \"");
    let report = devup_mcp_devup_ui::provenance::validate_fidelity(&s, "3997:46703", &o).unwrap();
    assert!(
        !report.assets.complete(),
        "a diagnostic label cannot substitute for emitted invisibility"
    );
    o.tsx = o
        .tsx
        .replace("visibility=\"unset \"", "visibility=\"hidden\"");
    s.nodes
        .get_mut("3997:46703")
        .unwrap()
        .fields
        .remove("absoluteRenderBounds");
    let report = devup_mcp_devup_ui::provenance::validate_fidelity(&s, "3997:46703", &o).unwrap();
    assert!(
        !report.assets.complete(),
        "missing raw evidence must not inherit a confirmed diagnostic"
    );
}

#[test]
fn r9_absolute_verification_is_rechecked_after_generated_css_changes() {
    let mut s = modal();
    let n = s.nodes.get_mut("3997:46621").unwrap();
    n.fields.extend(
        serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
            json!({"width":300,"x":30,"childrenIds":[]}),
        )
        .unwrap(),
    );
    let mut o = generate_component(
        &s,
        "3997:46582",
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    let previous = o.fidelity_report.impacts.approximated;
    assert!(
        o.diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_ABSOLUTE_VERIFIED"
                && d.fidelity_impact == Some(FidelityImpact::None))
    );
    assert!(o.tsx.contains("top=\"0px\""));
    o.tsx = o.tsx.replace("top=\"0px\"", "top=\"1px\"");
    let report = devup_mcp_devup_ui::provenance::validate_fidelity(&s, "3997:46582", &o).unwrap();
    assert_eq!(
        report.impacts.approximated,
        previous + 1,
        "stale verified diagnostic cannot certify changed CSS"
    );
}

#[test]
fn r9_legacy_exact_still_reports_and_rechecks_constraints() {
    for error in [false, true] {
        let mut s = modal();
        let n = s.nodes.get_mut("3997:46621").unwrap();
        n.fields.extend(serde_json::from_value::<serde_json::Map<String,serde_json::Value>>(json!({"width":300,"x":30,"childrenIds":[],"constraints":{"horizontal":"MAX","vertical":"MIN"}})).unwrap());
        if error {
            n.field_errors.insert("constraints".into(), "denied".into());
        }
        let o = generate_component(
            &s,
            "3997:46582",
            &CodegenOptions {
                inline_instances: true,
                ..Default::default()
            },
        )
        .unwrap();
        let d = o
            .diagnostics
            .iter()
            .find(|d| {
                d.node_id.as_deref() == Some("3997:46621")
                    && d.property.as_deref() == Some("layoutPositioning")
            })
            .expect("legacy exact must not bypass component evidence");
        assert_eq!(
            d.details.as_ref().unwrap()["components"]["horizontal"]["state"],
            if error { "approximated" } else { "verified" }
        );
        assert_eq!(
            d.fidelity_impact,
            Some(if error {
                FidelityImpact::Approximated
            } else {
                FidelityImpact::None
            })
        );
    }
}

#[test]
fn r9_parent_side_border_requires_containing_block_proof() {
    let mut s = modal();
    let p = s.nodes.get_mut("3997:46582").unwrap();
    p.fields.remove("strokeWeight");
    p.fields.extend(serde_json::from_value::<serde_json::Map<String,serde_json::Value>>(json!({"strokes":[{"type":"SOLID","visible":true,"color":{"r":0,"g":0,"b":0}}],"strokeLeftWeight":10,"strokeRightWeight":0,"strokeTopWeight":0,"strokeBottomWeight":0})).unwrap());
    let o = generate_component(
        &s,
        "3997:46582",
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(o.tsx.contains("borderLeft="));
    let d = o
        .diagnostics
        .iter()
        .find(|d| {
            d.node_id.as_deref() == Some("3997:46621")
                && d.property.as_deref() == Some("layoutPositioning")
        })
        .unwrap();
    assert_eq!(
        d.details.as_ref().unwrap()["components"]["horizontal"]["state"],
        "approximated"
    );
}

#[test]
fn r9_intrinsic_and_percentage_proofs_reject_read_errors_and_overrides() {
    for field in ["layoutMode", "maxWidth"] {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/wquw-151-proofread.json")).unwrap();
        let mut s: Snapshot = serde_json::from_value(fixture["snapshot"].clone()).unwrap();
        let n = s.nodes.get_mut("3879:35564").unwrap();
        if field == "layoutMode" {
            n.field_errors.insert(field.into(), "denied".into());
        } else {
            n.fields.insert(field.into(), json!(200));
        }
        let o = generate_component(
            &s,
            "3879:35518",
            &CodegenOptions {
                inline_instances: true,
                ..Default::default()
            },
        )
        .unwrap();
        let d = o
            .diagnostics
            .iter()
            .find(|d| {
                d.node_id.as_deref() == Some("3879:35564")
                    && d.property.as_deref() == Some("layoutPositioning")
            })
            .unwrap();
        assert_eq!(
            d.details.as_ref().unwrap()["components"]["width"]["state"],
            "approximated",
            "{field}"
        );
    }
}

#[test]
fn r9_center_relation_does_not_certify_mutated_parent_width() {
    let mut s = modal();
    s.nodes.get_mut("3997:46621").unwrap().fields.extend(
        serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
            json!({"width":300,"x":30,"childrenIds":[]}),
        )
        .unwrap(),
    );
    let mut o = generate_component(
        &s,
        "3997:46582",
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    o.tsx = o.tsx.replacen("w=\"360px\"", "w=\"460px\"", 1);
    let report = devup_mcp_devup_ui::provenance::validate_fidelity(&s, "3997:46582", &o).unwrap();
    assert!(
        report
            .uncovered_layout
            .iter()
            .any(|p| p == "3997:46582#width")
    );
    assert!(!report.strict_compatible());
}

#[test]
fn r9_verified_pixels_do_not_override_fill_asset_sizing() {
    let mut s: Snapshot =
        serde_json::from_str(include_str!("../../../fixtures/r9/non-rendering-node.json")).unwrap();
    s.nodes.get_mut("3997:46703").unwrap().fields.extend(serde_json::from_value::<serde_json::Map<String,serde_json::Value>>(json!({"absoluteRenderBounds":{"x":100,"y":0,"width":24,"height":24},"layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED"})).unwrap());
    let o = generate_component(&s, "3997:46703", &CodegenOptions::default()).unwrap();
    assert!(
        o.source_map
            .entries
            .iter()
            .any(|e| e.property.as_deref() == Some("width")
                && e.resolution == "verified-explicit-dimension")
    );
    s.nodes
        .get_mut("3997:46703")
        .unwrap()
        .fields
        .insert("layoutSizingHorizontal".into(), json!("FILL"));
    let report = devup_mcp_devup_ui::provenance::validate_fidelity(&s, "3997:46703", &o).unwrap();
    assert!(
        report
            .uncovered_layout
            .iter()
            .any(|p| p == "3997:46703#width")
    );
}
