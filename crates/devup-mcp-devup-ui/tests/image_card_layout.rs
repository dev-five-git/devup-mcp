use devup_mcp_devup_ui::{
    codegen::{CodegenOptions, CodegenOutput, generate_component, responsive::merge_breakpoints},
    provenance::attributes::{attribute_contract, uncovered_attributes},
};
use devup_mcp_figma::Snapshot;
use serde_json::{Value, json};

fn scene(mode: &str, sizes: Value, child_sizing: &str) -> Snapshot {
    let mut nodes = serde_json::Map::new();
    nodes.insert(
        "root".into(),
        json!({"id":"root","type":"FRAME","fields":{
            "name":"Cards", "width":400,"height":90,"layoutMode":mode,
            "inferredAutoLayout":{"layoutMode":mode,"itemSpacing":12},
            "layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED",
            "childrenIds":["wide","tall"],"gridColumnCount":2,"gridColumnSizes":sizes,
            "gridRowCount":1,"gridRowSizes":[{"type":"HUG"}],"gridColumnGap":12
        }}),
    );
    for (index, name) in ["wide", "tall"].into_iter().enumerate() {
        nodes.insert(
            name.into(),
            json!({"id":name,"type":"RECTANGLE","fields":{
                "name":name,"parentId":"root","width":160,"height":90,"isAsset":true,
                "layoutSizingHorizontal":child_sizing,"layoutSizingVertical":"FIXED",
                "gridColumnAnchorIndex":index,"gridRowAnchorIndex":0,
                "fills":[{"type":"IMAGE","visible":true,"scaleMode":"FILL","imageHash":name}]
            }}),
        );
    }
    serde_json::from_value(
        json!({"fileKey":"synthetic-cards","roots":["root"],"nodes":nodes,"diagnostics":[]}),
    )
    .unwrap()
}

fn generate(snapshot: &Snapshot) -> CodegenOutput {
    generate_component(snapshot, "root", &CodegenOptions::default()).unwrap()
}

fn tag<'a>(output: &'a CodegenOutput, id: &str) -> &'a str {
    let range = output
        .source_map
        .entries
        .iter()
        .find(|entry| entry.node_id.as_deref() == Some(id) && entry.property.is_none())
        .unwrap()
        .generated_range
        .as_ref()
        .unwrap();
    output.tsx[range.start..range.end]
        .split('>')
        .next()
        .unwrap()
}

fn assert_property_evidence(output: &CodegenOutput, id: &str, property: &str, emitted: &str) {
    assert!(
        output.diagnostics.iter().any(|d| {
            d.code == "DEVUP_CODEGEN_PROPERTY_EVIDENCE"
                && d.node_id.as_deref() == Some(id)
                && d.property.as_deref() == Some(property)
                && d.details.as_ref().is_some_and(|v| {
                    v["generatedProperty"] == emitted && v["propertyMappingVerified"] == true
                })
        }),
        "missing {id}#{property}: {:?}",
        output.diagnostics
    );
}

#[test]
fn fixed_image_cards_keep_css_dimensions_in_auto_layout() {
    let snapshot = scene("HORIZONTAL", Value::Null, "FIXED");
    let output = generate(&snapshot);
    assert!(!output.tsx.contains("<Grid"), "{}", output.tsx);
    for id in ["wide", "tall"] {
        assert!(tag(&output, id).contains("w=\"160px\""), "{}", output.tsx);
        assert!(tag(&output, id).contains("h=\"90px\""));
        assert!(!tag(&output, id).contains(" width="));
        assert!(!tag(&output, id).contains(" height="));
        assert!(
            output
                .source_map
                .entries
                .iter()
                .any(|e| e.node_id.as_deref() == Some(id)
                    && e.property.as_deref() == Some("width")
                    && e.generated_property.as_deref() == Some("w=\"160px\""))
        );
    }
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_IMAGE_INTRINSIC_LAYOUT")
    );
}

#[test]
fn flexible_columns_zero_the_content_minimum_without_changing_fixed_children() {
    let output = generate(&scene(
        "GRID",
        json!([{"type":"FLEX","value":1},{"type":"FLEX","value":1}]),
        "FIXED",
    ));
    let emitted = "gridTemplateColumns=\"repeat(2, minmax(0, 1fr))\"";
    assert!(tag(&output, "root").contains(emitted), "{}", output.tsx);
    assert!(tag(&output, "root").contains("gridTemplateRows=\"repeat(1, fit-content(100%))\""));
    for id in ["wide", "tall"] {
        assert!(tag(&output, id).contains("w=\"160px\""));
        assert!(tag(&output, id).contains("h=\"90px\""));
    }
    assert_property_evidence(&output, "root", "gridTemplateColumns", emitted);
    assert!(uncovered_attributes(&output.tsx, &attribute_contract(&output)).is_empty());
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_GRID_TRACK_SIZING_UNVERIFIED")
    );
}

#[test]
fn explicit_pixel_tracks_and_placement_spans_are_preserved() {
    let mut snapshot = scene(
        "GRID",
        json!([{"type":"FIXED","value":160},{"type":"FIXED","value":172}]),
        "FIXED",
    );
    let fields = &mut snapshot.nodes.get_mut("wide").unwrap().fields;
    fields.insert("gridColumnAnchorIndex".into(), json!(0));
    fields.insert("gridColumnSpan".into(), json!(2));
    fields.insert("gridRowSpan".into(), json!(2));
    let fields = &mut snapshot.nodes.get_mut("root").unwrap().fields;
    fields.insert("height".into(), json!(180));
    fields.insert("gridRowCount".into(), json!(2));
    fields.insert(
        "gridRowSizes".into(),
        json!([{"type":"HUG"},{"type":"HUG"}]),
    );
    snapshot
        .nodes
        .get_mut("tall")
        .unwrap()
        .fields
        .insert("gridRowAnchorIndex".into(), json!(1));
    let output = generate(&snapshot);
    let emitted = "gridTemplateColumns=\"160px 172px\"";
    assert!(tag(&output, "root").contains(emitted), "{}", output.tsx);
    assert!(tag(&output, "wide").contains("gridColumn=\"1 / span 2\""));
    assert!(tag(&output, "wide").contains("gridRow=\"1 / span 2\""));
    assert!(tag(&output, "tall").contains("gridColumn=\"2 / span 1\""));
    assert!(tag(&output, "tall").contains("gridRow=\"2 / span 1\""));
    assert!(tag(&output, "wide").contains("w=\"160px\""));
    assert_property_evidence(&output, "root", "gridTemplateColumns", emitted);
    assert_property_evidence(&output, "wide", "gridColumn", "gridColumn=\"1 / span 2\"");
}

#[test]
fn weighted_flexible_tracks_do_not_replace_hug_auto_or_fixed_tracks() {
    let snapshot = scene(
        "GRID",
        json!([
            {"type":"FLEX","value":2}, {"type":"FLEX","value":1},
            {"type":"HUG"}, {"type":"AUTO"}, {"type":"FIXED","value":160}
        ]),
        "HUG",
    );
    let output = generate(&snapshot);
    assert!(
        tag(&output, "root").contains(
            "gridTemplateColumns=\"minmax(0, 2fr) minmax(0, 1fr) fit-content(100%) auto 160px\""
        ),
        "{}",
        output.tsx
    );
    for id in ["wide", "tall"] {
        assert!(!tag(&output, id).contains("w=\"160px\""));
        let d = output
            .diagnostics
            .iter()
            .find(|d| {
                d.code == "DEVUP_CODEGEN_IMAGE_INTRINSIC_LAYOUT" && d.node_id.as_deref() == Some(id)
            })
            .unwrap();
        let details = d.details.as_ref().unwrap();
        assert_eq!(details["sourceImageSize"], Value::Null);
        assert_eq!(details["verification"]["state"], "unverified");
        assert_eq!(details["layout"]["zeroMinimumColumns"], true);
        assert_eq!(details["generatedSizing"], json!(["h=\"90px\""]));
    }
}

#[test]
fn hug_grids_retain_intrinsic_minimum_and_count_only_hug_uses_auto() {
    let mut snapshot = scene(
        "GRID",
        json!([{"type":"FLEX","value":1},{"type":"HUG"}]),
        "HUG",
    );
    snapshot
        .nodes
        .get_mut("root")
        .unwrap()
        .fields
        .insert("layoutSizingHorizontal".into(), json!("HUG"));
    let output = generate(&snapshot);
    assert!(
        tag(&output, "root").contains("gridTemplateColumns=\"1fr fit-content(100%)\""),
        "{}",
        output.tsx
    );
    snapshot
        .nodes
        .get_mut("root")
        .unwrap()
        .fields
        .remove("gridColumnSizes");
    let output = generate(&snapshot);
    assert!(tag(&output, "root").contains("gridTemplateColumns=\"repeat(2, auto)\""));
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_GRID_TRACK_SIZING_UNVERIFIED")
    );
}

#[test]
fn absent_unknown_and_invalid_tracks_are_truthfully_diagnosed() {
    for (sizes, expected, reason) in [
        (
            Value::Null,
            "repeat(2, minmax(0, 1fr))",
            "track-sizes-not-collected",
        ),
        (
            json!([{"type":"UNKNOWN","value":160},{"type":"FIXED"}]),
            "repeat(2, auto)",
            "unknown-or-invalid-track-size",
        ),
        (
            json!([{"type":"FLEX","value":-2},{"type":"FIXED","value":-10}]),
            "repeat(2, auto)",
            "unknown-or-invalid-track-size",
        ),
        (
            json!([{"type":"FIXED","value":160}]),
            "repeat(1, 160px)",
            "track-count-mismatch",
        ),
    ] {
        let output = generate(&scene("GRID", sizes, "FIXED"));
        let emitted = format!("gridTemplateColumns=\"{expected}\"");
        assert!(tag(&output, "root").contains(&emitted), "{}", output.tsx);
        let d = output
            .diagnostics
            .iter()
            .find(|d| {
                d.code == "DEVUP_CODEGEN_GRID_TRACK_SIZING_UNVERIFIED"
                    && d.property.as_deref() == Some("gridTemplateColumns")
            })
            .unwrap();
        let details = d.details.as_ref().unwrap();
        assert!(
            details["reasonCodes"]
                .as_array()
                .unwrap()
                .contains(&json!(reason))
        );
        assert_eq!(details["generatedProperty"], emitted);
        assert_eq!(details["verification"]["state"], "unverified");
        assert_property_evidence(&output, "root", "gridTemplateColumns", &emitted);
    }
}

#[test]
fn missing_track_count_omits_empty_css_and_keeps_diagnostic() {
    let mut snapshot = scene("GRID", Value::Null, "HUG");
    snapshot
        .nodes
        .get_mut("root")
        .unwrap()
        .fields
        .remove("gridColumnCount");
    let output = generate(&snapshot);
    assert!(!tag(&output, "root").contains("gridTemplateColumns="));
    let d = output
        .diagnostics
        .iter()
        .find(|d| {
            d.code == "DEVUP_CODEGEN_GRID_TRACK_SIZING_UNVERIFIED"
                && d.property.as_deref() == Some("gridTemplateColumns")
        })
        .unwrap();
    assert_eq!(
        d.details.as_ref().unwrap()["generatedProperty"],
        Value::Null
    );
}

#[test]
fn invalid_and_oversized_track_counts_do_not_allocate_or_fabricate_tracks() {
    for count in [json!(-1), json!(1.5), json!("2"), json!(1e20)] {
        let mut snapshot = scene("GRID", Value::Null, "FIXED");
        snapshot
            .nodes
            .get_mut("root")
            .unwrap()
            .fields
            .insert("gridColumnCount".into(), count);
        let output = generate(&snapshot);
        assert!(!tag(&output, "root").contains("gridTemplateColumns="));
        assert!(
            output
                .diagnostics
                .iter()
                .any(|d| d.code == "DEVUP_CODEGEN_GRID_TRACK_SIZING_UNVERIFIED"
                    && d.property.as_deref() == Some("gridTemplateColumns"))
        );
    }
    let snapshot = scene(
        "GRID",
        json!(vec![json!({"type":"FLEX","value":1}); 4097]),
        "FIXED",
    );
    let output = generate(&snapshot);
    assert!(!tag(&output, "root").contains("gridTemplateColumns="));
    let d = output
        .diagnostics
        .iter()
        .find(|d| {
            d.code == "DEVUP_CODEGEN_GRID_TRACK_SIZING_UNVERIFIED"
                && d.property.as_deref() == Some("gridTemplateColumns")
        })
        .unwrap();
    assert_eq!(
        d.details.as_ref().unwrap()["reasonCodes"],
        json!(["track-list-exceeds-codegen-budget"])
    );
}

#[test]
fn flexible_auto_layout_reports_potential_intrinsic_dependency_without_guessing_size() {
    let output = generate(&scene("HORIZONTAL", Value::Null, "FILL"));
    assert!(!output.tsx.contains("<Grid"));
    for id in ["wide", "tall"] {
        assert!(tag(&output, id).contains("flex=\"1\""));
        assert!(tag(&output, id).contains("h=\"90px\""));
        let d = output
            .diagnostics
            .iter()
            .find(|d| {
                d.code == "DEVUP_CODEGEN_IMAGE_INTRINSIC_LAYOUT" && d.node_id.as_deref() == Some(id)
            })
            .unwrap();
        let details = d.details.as_ref().unwrap();
        assert_eq!(details["sourceImageSize"], Value::Null);
        assert_eq!(details["layout"]["mode"], "HORIZONTAL");
        assert!(
            details["generatedSizing"]
                .as_array()
                .unwrap()
                .contains(&json!("flex=\"1\""))
        );
        assert!(d.message.contains("may affect"));
    }
}

#[test]
fn safe_grid_with_explicit_dimensions_or_source_ratio_already_reserves_image_space() {
    for ratio in [false, true] {
        let mut snapshot = scene("GRID", json!([{"type":"FLEX"},{"type":"FLEX"}]), "FILL");
        if ratio {
            for id in ["wide", "tall"] {
                let fields = &mut snapshot.nodes.get_mut(id).unwrap().fields;
                fields.insert("layoutSizingVertical".into(), json!("HUG"));
                fields.insert("targetAspectRatio".into(), json!({"x":16,"y":9}));
            }
        }
        let output = generate(&snapshot);
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|d| d.code == "DEVUP_CODEGEN_IMAGE_INTRINSIC_LAYOUT"),
            "{:?}",
            output.diagnostics
        );
        for id in ["wide", "tall"] {
            assert!(tag(&output, id).contains("w=\"100%\""));
            if ratio {
                assert!(tag(&output, id).contains("aspectRatio="));
                assert!(!tag(&output, id).contains("h="));
            } else {
                assert!(tag(&output, id).contains("h=\"90px\""));
            }
            assert!(!tag(&output, id).contains(" width="));
        }
    }
}

#[test]
fn omitted_or_absolute_images_and_non_flex_boxes_do_not_claim_intrinsic_flex_risk() {
    for mode in ["hidden", "absolute", "box"] {
        let mut snapshot = scene("HORIZONTAL", Value::Null, "HUG");
        for id in ["wide", "tall"] {
            let fields = &mut snapshot.nodes.get_mut(id).unwrap().fields;
            if mode == "hidden" {
                fields.insert("visible".into(), json!(false));
            } else if mode == "absolute" {
                fields.insert("layoutPositioning".into(), json!("ABSOLUTE"));
            }
        }
        if mode == "box" {
            snapshot
                .nodes
                .get_mut("root")
                .unwrap()
                .fields
                .remove("inferredAutoLayout");
        }
        let output = generate(&snapshot);
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|d| d.code == "DEVUP_CODEGEN_IMAGE_INTRINSIC_LAYOUT"),
            "{mode}: {:?}",
            output.diagnostics
        );
    }
}

#[test]
fn responsive_grids_merge_safe_fractional_and_explicit_tracks_with_evidence() {
    let mut snapshot = scene(
        "GRID",
        json!([{"type":"FLEX","value":1},{"type":"FLEX","value":1}]),
        "FIXED",
    );
    snapshot.nodes.clear();
    snapshot.roots.clear();
    for (index, (width, name, sizes)) in [
        (360, "mobile", json!([{"type":"FLEX","value":1}])),
        (
            900,
            "tablet",
            json!([{"type":"FLEX","value":1},{"type":"FLEX","value":1}]),
        ),
        (
            1440,
            "desktop",
            json!([{"type":"FIXED","value":160},{"type":"FIXED","value":160}]),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut frame = scene("GRID", sizes.clone(), "FIXED");
        let root = format!("root{index}");
        snapshot.roots.push(root.clone());
        for (_, mut node) in std::mem::take(&mut frame.nodes) {
            let is_root = node.id == "root";
            node.id = format!("{}{index}", node.id);
            if is_root {
                node.fields.insert("name".into(), json!(name));
                node.fields.insert("width".into(), json!(width));
                node.fields.insert(
                    "gridColumnCount".into(),
                    json!(sizes.as_array().unwrap().len()),
                );
                node.fields.insert(
                    "childrenIds".into(),
                    json!([format!("wide{index}"), format!("tall{index}")]),
                );
            } else {
                node.fields.insert("parentId".into(), json!(root));
            }
            snapshot.nodes.insert(node.id.clone(), node);
        }
    }
    let merged = merge_breakpoints(&snapshot, &CodegenOptions::default())
        .unwrap()
        .unwrap();
    assert!(
        merged.tsx.contains("gridTemplateColumns={["),
        "{}",
        merged.tsx
    );
    for expected in [
        "repeat(1, minmax(0, 1fr))",
        "repeat(2, minmax(0, 1fr))",
        "repeat(2, 160px)",
    ] {
        assert!(merged.tsx.contains(expected), "{}", merged.tsx);
    }
    assert!(merged.gaps.is_empty(), "{:?}", merged.gaps);
    assert!(merged.verified);
}
