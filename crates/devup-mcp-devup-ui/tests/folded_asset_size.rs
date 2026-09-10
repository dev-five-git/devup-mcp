use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::{SnapshotChunk, merge_chunks};
use serde_json::json;

#[test]
fn fixed_non_square_frame_folded_into_mask_keeps_its_size() {
    let chunk: SnapshotChunk = serde_json::from_value(json!({
        "fileKey": "file-key",
        "version": "1",
        "rootIds": ["1:root"],
        "nodes": [
            {
                "id": "1:root", "type": "FRAME",
                "fields": {
                    "name": "Screen", "childrenIds": ["1:logo"],
                    "width": 100, "height": 100,
                    "fills": [{
                        "type": "SOLID", "visible": true,
                        "color": {"r": 1, "g": 1, "b": 1}
                    }]
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:logo", "type": "FRAME",
                "fields": {
                    "name": "BI Logo", "parentId": "1:root", "childrenIds": ["1:vector"],
                    "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                    "layoutPositioning": "ABSOLUTE", "width": 24, "height": 9,
                    "x": 64, "y": 79,
                    "targetAspectRatio": {"x": 79.9, "y": 29.9}
                },
                "extra": {}, "fieldErrors": {}
            },
            {
                "id": "1:vector", "type": "VECTOR",
                "fields": {
                    "name": "BI Logo Vector", "parentId": "1:logo", "childrenIds": [],
                    "fills": [{
                        "type": "SOLID", "visible": true,
                        "color": {"r": 0, "g": 0, "b": 0}
                    }]
                },
                "extra": {}, "fieldErrors": {}
            }
        ],
        "diagnostics": []
    }))
    .expect("synthetic snapshot");
    let snapshot = merge_chunks(vec![chunk]).expect("snapshot");

    let tsx = generate_component(&snapshot, "1:root", &CodegenOptions::default())
        .expect("codegen")
        .tsx;

    assert!(tsx.contains("maskImage=\"url('/icons/BI Logo.svg')\""));
    assert!(
        tsx.contains("h=\"9px\"") && tsx.contains("w=\"24px\""),
        "folded mask lost its fixed dimensions:\n{tsx}"
    );
}

fn wquw119() -> devup_mcp_figma::Snapshot {
    serde_json::from_str(include_str!("../../../fixtures/wquw-119-snapshot.json")).unwrap()
}

#[test]
fn r5_wquw119_hug_mask_preserves_render_size_and_source_map() {
    let snapshot = wquw119();
    let node = &snapshot.nodes["3997:46317"];
    assert_eq!(node.fields["height"], 48);
    assert_eq!(node.fields["absoluteRenderBounds"]["width"], 320);
    assert_eq!(node.fields["childrenIds"], json!(["3997:46318"]));
    let output = generate_component(&snapshot, "3997:46315", &CodegenOptions::default()).unwrap();
    for (axis, expected) in [
        ("width", "w=\"100%\""),
        ("height", "aspectRatio=\"320 / 48\""),
    ] {
        assert!(
            output.source_map.entries.iter().any(|e| {
                e.node_id.as_deref() == Some("3997:46317")
                    && e.property.as_deref() == Some(axis)
                    && e.generated_range
                        .as_ref()
                        .is_some_and(|r| output.tsx.get(r.start..r.end) == Some(expected))
            }),
            "missing {axis} evidence: {}",
            output.tsx
        );
    }
    assert!(!output.tsx.contains("w=\"320px\""));
    assert!(
        output
            .source_map
            .entries
            .iter()
            .any(|e| e.node_id.as_deref() == Some("3997:46317")
                && e.property.as_deref() == Some("childrenIds")
                && e.resolution == "restored-hug-after-mask-child-folding")
    );
    // At the captured width and two different host widths, the emitted ratio
    // preserves the SVG proportions rather than retaining a 48px height.
    let ratio = output
        .tsx
        .split("aspectRatio=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let (w, h) = ratio.split_once('/').unwrap();
    let ratio = w.trim().parse::<f64>().unwrap() / h.trim().parse::<f64>().unwrap();
    for (parent_width, expected_height) in [(160.0, 24.0), (320.0, 48.0), (640.0, 96.0)] {
        assert!((parent_width / ratio - expected_height).abs() < 1e-9);
    }
    assert!(
        !output
            .fidelity_report
            .uncovered_layout
            .iter()
            .any(|p| p.starts_with("3997:46317#"))
    );
}

#[test]
fn r5_hug_asset_missing_geometry_is_not_exact() {
    let mut snapshot = wquw119();
    let node = snapshot.nodes.get_mut("3997:46317").unwrap();
    node.fields.remove("height");
    node.fields.remove("absoluteBoundingBox");
    let output = generate_component(&snapshot, "3997:46315", &CodegenOptions::default()).unwrap();
    assert!(!output.fidelity_report.strict_compatible());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.node_id.as_deref() == Some("3997:46317")
                && d.property.as_deref() == Some("height")
                && d.code == "DEVUP_CODEGEN_LAYOUT_UNCOVERED")
    );
}

#[test]
fn r5_asset_size_mapping_to_zero_is_not_evidence() {
    let snapshot = wquw119();
    let mut output =
        generate_component(&snapshot, "3997:46315", &CodegenOptions::default()).unwrap();
    // Preserve byte offsets to isolate value validation from range validity.
    output.tsx = output.tsx.replace("320 / 48", "320 / 00");
    let report =
        devup_mcp_devup_ui::provenance::validate_fidelity(&snapshot, "3997:46315", &output)
            .unwrap();
    assert!(
        report
            .uncovered_layout
            .contains(&"3997:46317#height".to_owned())
    );
}

#[test]
fn r5_hug_asset_wrong_positive_height_is_not_exact() {
    let snapshot = wquw119();
    let mut output =
        generate_component(&snapshot, "3997:46315", &CodegenOptions::default()).unwrap();
    output.tsx = output.tsx.replace("320 / 48", "320 / 49");
    let report =
        devup_mcp_devup_ui::provenance::validate_fidelity(&snapshot, "3997:46315", &output)
            .unwrap();
    assert!(
        report
            .uncovered_layout
            .contains(&"3997:46317#height".to_owned())
    );
}

#[test]
fn r5_absolute_wide_hug_mask_does_not_collapse() {
    let mut snapshot = wquw119();
    let node = snapshot.nodes.get_mut("3997:46317").unwrap();
    node.fields
        .insert("layoutPositioning".into(), json!("ABSOLUTE"));
    node.fields.remove("absoluteRenderBounds");
    node.fields.remove("absoluteBoundingBox");
    let parent = snapshot.nodes.get_mut("3997:46316").unwrap();
    parent.fields.insert("width".into(), json!(300));
    let output = generate_component(&snapshot, "3997:46315", &CodegenOptions::default()).unwrap();
    assert!(
        output.tsx.contains("aspectRatio=\"320 / 48\""),
        "{}",
        output.tsx
    );
}

#[test]
fn r5_empty_non_asset_hug_box_cannot_claim_visible_height() {
    let mut snapshot = wquw119();
    let node = snapshot.nodes.get_mut("3997:46317").unwrap();
    node.fields.insert("childrenIds".into(), json!([]));
    node.fields.insert(
        "fills".into(),
        json!([{"type":"SOLID","visible":true,"color":{"r":1,"g":0,"b":0}}]),
    );
    let output = generate_component(&snapshot, "3997:46315", &CodegenOptions::default()).unwrap();
    assert!(
        output
            .fidelity_report
            .uncovered_layout
            .contains(&"3997:46317#height".to_owned())
    );
}

#[test]
fn r5_asset_url_identity_cannot_be_replaced_with_another_file() {
    let snapshot = wquw119();
    let mut output =
        generate_component(&snapshot, "3997:46315", &CodegenOptions::default()).unwrap();
    assert_eq!(output.fidelity_report.assets.covered, 1);
    output.tsx = output.tsx.replace(".svg", ".png");
    assert_eq!(
        devup_mcp_devup_ui::provenance::validate_fidelity(&snapshot, "3997:46315", &output)
            .unwrap()
            .assets
            .covered,
        0
    );
}

#[test]
fn r5_embedded_hug_asset_reports_host_size_without_pinning_root() {
    let snapshot = wquw119();
    let output = generate_component(
        &snapshot,
        "3997:46317",
        &CodegenOptions {
            root_layout: devup_mcp_devup_ui::codegen::RootLayout::Embedded,
            ..CodegenOptions::default()
        },
    )
    .unwrap();
    assert!(!output.tsx.contains("h=\"48px\""));
    assert!(
        output
            .fidelity_report
            .uncovered_layout
            .contains(&"3997:46317#height".to_owned())
    );
}

#[test]
fn r5_asset_url_in_non_rendering_attribute_is_not_covered() {
    let snapshot = wquw119();
    let mut output =
        generate_component(&snapshot, "3997:46315", &CodegenOptions::default()).unwrap();
    output.tsx = output.tsx.replace("maskImage=", "data-icon=");
    assert_eq!(
        devup_mcp_devup_ui::provenance::validate_fidelity(&snapshot, "3997:46315", &output)
            .unwrap()
            .assets
            .covered,
        0
    );
}

#[test]
fn r5_empty_hug_non_asset_cannot_use_percentage_or_wrong_size_as_proof() {
    let mut snapshot = wquw119();
    let node = snapshot.nodes.get_mut("3997:46317").unwrap();
    node.fields.insert("childrenIds".into(), json!([]));
    node.fields.insert(
        "fills".into(),
        json!([{"type":"SOLID","visible":true,"color":{"r":1,"g":0,"b":0}}]),
    );
    node.fields
        .insert("layoutSizingVertical".into(), json!("FIXED"));
    let output = generate_component(&snapshot, "3997:46315", &CodegenOptions::default()).unwrap();
    assert!(output.tsx.contains("h=\"48px\""));
    for bad in ["49px", "100%"] {
        let mut wrong = output.clone();
        wrong.tsx = wrong.tsx.replace("h=\"48px\"", &format!("h=\"{bad}\""));
        let report =
            devup_mcp_devup_ui::provenance::validate_fidelity(&snapshot, "3997:46315", &wrong)
                .unwrap();
        assert!(
            report
                .uncovered_layout
                .contains(&"3997:46317#height".to_owned())
        );
    }
}

#[test]
fn r5_ratio_without_fill_anchor_or_with_fixed_fill_width_is_not_exact() {
    let snapshot = wquw119();
    let output = generate_component(&snapshot, "3997:46315", &CodegenOptions::default()).unwrap();
    for replacement in ["x=\"100%\"", "w=\"32px\""] {
        let mut wrong = output.clone();
        wrong.tsx = wrong.tsx.replace("w=\"100%\"", replacement);
        let report =
            devup_mcp_devup_ui::provenance::validate_fidelity(&snapshot, "3997:46315", &wrong)
                .unwrap();
        assert!(
            report
                .uncovered_layout
                .contains(&"3997:46317#height".to_owned())
        );
        assert!(
            report
                .uncovered_layout
                .contains(&"3997:46317#width".to_owned())
        );
    }
}

#[test]
fn r5_both_hug_mask_uses_measured_anchors_and_records_the_lost_children() {
    let mut snapshot = wquw119();
    snapshot
        .nodes
        .get_mut("3997:46317")
        .unwrap()
        .fields
        .insert("layoutSizingHorizontal".into(), json!("HUG"));
    let output = generate_component(&snapshot, "3997:46315", &CodegenOptions::default()).unwrap();
    assert!(output.tsx.contains("w=\"320px\""));
    assert!(output.tsx.contains("h=\"48px\""));
    assert!(
        !output
            .fidelity_report
            .uncovered_layout
            .iter()
            .any(|p| p.starts_with("3997:46317#"))
    );
}

#[test]
fn r5_vertical_fill_requires_definite_parent_height() {
    let mut snapshot = wquw119();
    let node = snapshot.nodes.get_mut("3997:46317").unwrap();
    node.fields
        .insert("layoutSizingHorizontal".into(), json!("HUG"));
    node.fields
        .insert("layoutSizingVertical".into(), json!("FILL"));
    let parent = snapshot.nodes.get_mut("3997:46316").unwrap();
    parent
        .fields
        .insert("layoutSizingVertical".into(), json!("HUG"));
    let output = generate_component(&snapshot, "3997:46315", &CodegenOptions::default()).unwrap();
    assert!(output.tsx.contains("aspectRatio=\"320 / 48\""));
    assert!(
        output
            .fidelity_report
            .uncovered_layout
            .contains(&"3997:46317#height".to_owned())
    );
    assert!(
        output
            .fidelity_report
            .uncovered_layout
            .contains(&"3997:46317#width".to_owned())
    );
}

#[test]
fn r5_fixed_mask_never_receives_hug_repair_provenance() {
    let mut snapshot = wquw119();
    snapshot
        .nodes
        .get_mut("3997:46317")
        .unwrap()
        .fields
        .insert("layoutSizingVertical".into(), json!("FIXED"));
    let output = generate_component(&snapshot, "3997:46315", &CodegenOptions::default()).unwrap();
    assert!(
        !output
            .source_map
            .entries
            .iter()
            .any(|e| e.resolution.starts_with("restored-hug"))
    );
    assert!(!output.tsx.contains("w=\"320px\""));
}

#[test]
fn r5_fill_ratio_cannot_bootstrap_a_hug_parent_width() {
    let mut snapshot = wquw119();
    let parent = snapshot.nodes.get_mut("3997:46316").unwrap();
    parent
        .fields
        .insert("layoutSizingHorizontal".into(), json!("HUG"));
    let output = generate_component(&snapshot, "3997:46315", &CodegenOptions::default()).unwrap();
    assert!(
        output
            .fidelity_report
            .uncovered_layout
            .contains(&"3997:46317#height".to_owned())
    );
}

#[test]
fn r5_mask_ratio_keeps_fractional_geometry_precision() {
    let mut snapshot = wquw119();
    let node = snapshot.nodes.get_mut("3997:46317").unwrap();
    node.fields.insert("width".into(), json!(320.123));
    node.fields.insert("height".into(), json!(48.234));
    node.fields.remove("absoluteBoundingBox");
    node.fields.remove("absoluteRenderBounds");
    let output = generate_component(&snapshot, "3997:46315", &CodegenOptions::default()).unwrap();
    assert!(output.tsx.contains("aspectRatio=\"320.123 / 48.234\""));
    assert!(
        !output
            .fidelity_report
            .uncovered_layout
            .iter()
            .any(|p| p.starts_with("3997:46317#"))
    );
}

#[test]
fn r5_rotated_or_offset_mask_cannot_claim_responsive_repair() {
    for rotated in [true, false] {
        let mut snapshot = wquw119();
        let node = snapshot.nodes.get_mut("3997:46317").unwrap();
        if rotated {
            node.fields.insert("rotation".into(), json!(90));
            node.fields.get_mut("absoluteBoundingBox").unwrap()["width"] = json!(48);
            node.fields.get_mut("absoluteBoundingBox").unwrap()["height"] = json!(320);
        } else {
            node.fields.get_mut("absoluteRenderBounds").unwrap()["width"] = json!(300);
        }
        let output =
            generate_component(&snapshot, "3997:46315", &CodegenOptions::default()).unwrap();
        assert!(!output.tsx.contains("aspectRatio=\"320 / 48\""));
        assert!(
            !output
                .source_map
                .entries
                .iter()
                .any(|e| e.resolution.starts_with("restored-hug"))
        );
        assert!(!output.fidelity_report.strict_compatible());
    }
}
