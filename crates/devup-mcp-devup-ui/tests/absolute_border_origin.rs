use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::json;

fn source(mobile: bool) -> (Snapshot, &'static str, &'static str) {
    let mut snapshot: Snapshot =
        serde_json::from_str(include_str!("fixtures/vision-border-origin.json")).unwrap();
    let (parent, child) = if mobile {
        ("44:3235", "I44:3235;44:3700")
    } else {
        ("44:2832", "I44:2832;44:3209")
    };
    snapshot.roots = vec![parent.into()];
    snapshot.nodes.retain(|id, _| id == parent || id == child);
    // A live sibling keeps the parent in the native layout path, as its real
    // captured card does. The parent/ellipse paint and geometry remain exact.
    snapshot
        .nodes
        .get_mut(parent)
        .unwrap()
        .fields
        .insert("childrenIds".into(), json!([child, "label"]));
    snapshot.nodes.insert("label".into(), serde_json::from_value(json!({"id":"label","type":"TEXT","fields":{
        "parentId":parent,"characters":"Live sibling","fontSize":16,"textAutoResize":"WIDTH_AND_HEIGHT"}})).unwrap());
    (snapshot, parent, child)
}

#[test]
fn captured_leading_border_origin_is_subtracted_and_explained() {
    for mobile in [false, true] {
        let (snapshot, parent, _) = source(mobile);
        let output = generate_component(
            &snapshot,
            parent,
            &CodegenOptions {
                inline_instances: true,
                ..Default::default()
            },
        )
        .unwrap();
        let expected = if mobile { "28px" } else { "33px" };
        assert!(
            output.tsx.contains(&format!("top=\"{expected}\"")),
            "{}",
            output.tsx
        );
        assert!(
            output.tsx.contains(if mobile {
                "left=\"-10px\""
            } else {
                "left=\"-18px\""
            }),
            "{}",
            output.tsx
        );
        assert!(
            output.source_map.entries.iter().any(|entry| entry
                .calculation
                .as_deref()
                .is_some_and(|calculation| calculation.contains("native parent")
                    && calculation.contains("leading border"))),
            "{:?}",
            output.source_map
        );
        if !mobile && let Ok(path) = std::env::var("DEVUP_BORDER_ORIGIN_PROBE_OUTPUT") {
            std::fs::write(path, output.tsx).unwrap();
        }
    }
}

#[test]
fn outline_hidden_none_unknown_and_non_min_paths_are_not_rewritten() {
    for variant in [
        "outline",
        "hidden-paint",
        "hidden-parent",
        "none-parent",
        "unknown-stroke",
        "unknown-constraint",
        "center",
        "rotation",
    ] {
        let (mut snapshot, parent, child) = source(false);
        match variant {
            "outline" => {
                let fields = &mut snapshot.nodes.get_mut(parent).unwrap().fields;
                fields.insert("strokeWeight".into(), json!(1));
                fields.insert("strokeAlign".into(), json!("OUTSIDE"));
            }
            "hidden-paint" => {
                snapshot
                    .nodes
                    .get_mut(parent)
                    .unwrap()
                    .fields
                    .get_mut("strokes")
                    .unwrap()[0]["visible"] = json!(false)
            }
            "hidden-parent" => {
                snapshot
                    .nodes
                    .get_mut(parent)
                    .unwrap()
                    .fields
                    .insert("visible".into(), json!(false));
            }
            "none-parent" => {
                snapshot
                    .nodes
                    .get_mut(parent)
                    .unwrap()
                    .fields
                    .insert("layoutMode".into(), json!("NONE"));
            }
            "unknown-stroke" => {
                snapshot
                    .nodes
                    .get_mut(parent)
                    .unwrap()
                    .field_errors
                    .insert("strokes".into(), "read failed".into());
            }
            "unknown-constraint" => {
                snapshot
                    .nodes
                    .get_mut(child)
                    .unwrap()
                    .fields
                    .insert("constraints".into(), json!({"$unsupported":"unknown"}));
            }
            "center" => {
                snapshot
                    .nodes
                    .get_mut(child)
                    .unwrap()
                    .fields
                    .get_mut("constraints")
                    .unwrap()["vertical"] = json!("CENTER")
            }
            "rotation" => {
                snapshot
                    .nodes
                    .get_mut(child)
                    .unwrap()
                    .fields
                    .insert("rotation".into(), json!(20));
            }
            _ => unreachable!(),
        }
        let output = generate_component(
            &snapshot,
            parent,
            &CodegenOptions {
                inline_instances: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            !output.tsx.contains("top=\"33px\""),
            "{variant}: {}",
            output.tsx
        );
        assert!(
            !output.source_map.entries.iter().any(|entry| entry
                .calculation
                .as_deref()
                .is_some_and(|v| v.contains("border-origin conversion"))),
            "{variant}"
        );
    }
}

#[test]
fn semantic_float_identity_transform_gets_the_same_border_conversion() {
    let (mut snapshot, parent, child) = source(false);
    for id in [parent, child] {
        let transform = snapshot
            .nodes
            .get_mut(id)
            .unwrap()
            .fields
            .get_mut("relativeTransform")
            .unwrap();
        transform[0][0] = json!(1.0);
        transform[0][1] = json!(0.0);
        transform[1][0] = json!(0.0);
        transform[1][1] = json!(1.0);
    }
    let output = generate_component(
        &snapshot,
        parent,
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(output.tsx.contains("top=\"33px\""), "{}", output.tsx);
}
