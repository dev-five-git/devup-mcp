use devup_mcp_devup_ui::codegen::{CodegenOptions, CodegenOutput, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::{Value, json};

fn generate(edge: usize, align: &str, extra: Value) -> CodegenOutput {
    let mut fields = json!({
        "layoutMode":"VERTICAL", "inferredAutoLayout":{"layoutMode":"VERTICAL"}, "layoutSizingHorizontal":"FIXED",
        "layoutSizingVertical":"HUG", "width":328, "height":319,
        "paddingTop":0,"paddingRight":0,"paddingBottom":0,"paddingLeft":0,
        "strokeWeight":{"$unsupported":"symbol"}, "strokeAlign":align,
        "strokeTopWeight":0,"strokeRightWeight":0,"strokeBottomWeight":0,"strokeLeftWeight":0,
        "strokes":[{"type":"SOLID","color":{"r":1,"g":0,"b":0}}],
        "childrenIds":["child"]
    });
    fields[[
        "strokeTopWeight",
        "strokeRightWeight",
        "strokeBottomWeight",
        "strokeLeftWeight",
    ][edge]] = json!(2);
    fields
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let snapshot: Snapshot = serde_json::from_value(json!({
        "fileKey":"edge","roots":["root"],"diagnostics":[],"nodes":{
            "root":{"id":"root","type":"FRAME","fields":fields},
            "child":{"id":"child","type":"TEXT","fields":{
                "parentId":"root","characters":"content","width":100,"height":40
            }}
        }
    }))
    .unwrap();
    generate_component(&snapshot, "root", &CodegenOptions::default()).unwrap()
}

#[test]
fn single_outside_edge_paints_without_consuming_layout_space() {
    for (align, shadows) in [(
        "OUTSIDE",
        [
            "0 -2px 0 0 #F00",
            "2px 0 0 0 #F00",
            "0 2px 0 0 #F00",
            "-2px 0 0 0 #F00",
        ],
    )] {
        for (edge, shadow) in shadows.iter().enumerate() {
            for padding in [0, 80] {
                let output = generate(
                    edge,
                    align,
                    json!({"paddingTop":padding,"paddingBottom":padding}),
                );
                assert!(
                    output.tsx.contains(&format!("boxShadow=\"{shadow}\"")),
                    "{}",
                    output.tsx
                );
                assert!(
                    !output.tsx.contains("borderTop=")
                        && !output.tsx.contains("borderBottom=")
                        && !output.tsx.contains("borderLeft=")
                        && !output.tsx.contains("borderRight="),
                    "{}",
                    output.tsx
                );
                if padding > 0 {
                    assert!(output.tsx.contains("py=\"80px\""), "{}", output.tsx);
                }
            }
        }
    }
}

#[test]
fn edge_paint_and_real_shadow_both_survive() {
    let output = generate(
        0,
        "OUTSIDE",
        json!({"effects":[{
            "type":"DROP_SHADOW","offset":{"x":3,"y":4},"radius":5,"spread":0,
            "color":{"r":0,"g":0,"b":0,"a":1}
        }]}),
    );
    assert!(
        output
            .tsx
            .contains("boxShadow=\"0 -2px 0 0 #F00, 3px 4px 5px 0 #000\""),
        "{}",
        output.tsx
    );
    assert_eq!(output.tsx.matches("boxShadow=").count(), 1);
}

#[test]
fn unsupported_edge_shapes_keep_existing_emission() {
    for extra in [
        json!({"cornerRadius":8}),
        json!({"topLeftRadius":8}),
        json!({"dashPattern":[2,2]}),
        json!({"strokeBottomWeight":1}),
        json!({"strokeRightWeight":null}),
    ] {
        let output = generate(0, "OUTSIDE", extra);
        assert!(!output.tsx.contains("boxShadow="), "{}", output.tsx);
    }
    for align in ["INSIDE", "CENTER"] {
        let output = generate(0, align, json!({}));
        assert!(output.tsx.contains("borderTop="), "{}", output.tsx);
    }
}

#[test]
fn derived_edge_paint_maps_to_stroke_data_instead_of_inventing_an_effect() {
    let output = generate(0, "OUTSIDE", json!({}));
    let entry = output
        .source_map
        .entries
        .iter()
        .find(|entry| {
            entry
                .generated_property
                .as_deref()
                .is_some_and(|prop| prop.starts_with("boxShadow="))
        })
        .expect("edge paint source map");
    assert_eq!(entry.property.as_deref(), Some("strokes"));
    assert_eq!(entry.resolution, "derived-single-edge-outside-stroke");
}

#[test]
fn an_unpaintable_selected_stroke_does_not_claim_a_real_effect_as_stroke_paint() {
    let output = generate(
        0,
        "OUTSIDE",
        json!({
            "strokes":[{"type":"SOLID"},{"type":"SOLID","color":{"r":1,"g":0,"b":0}}],
            "effects":[{"type":"DROP_SHADOW","offset":{"x":3,"y":4},"radius":5,
                "color":{"r":0,"g":0,"b":0,"a":1}}]
        }),
    );
    assert!(
        !output
            .source_map
            .entries
            .iter()
            .any(|entry| entry.resolution == "derived-single-edge-outside-stroke")
    );
    assert!(
        output.tsx.contains("boxShadow=\"3px 4px 5px 0 #000\""),
        "{}",
        output.tsx
    );
}

#[test]
fn exported_asset_boundary_is_not_treated_as_a_live_css_layout_container() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../fixtures/devup-figma-plugin/cases/codegen/upstream-codegen-181-5afa626d49.json"
    ))
    .unwrap();
    let snapshot: Snapshot =
        serde_json::from_value(fixture["payload"]["snapshot"].clone()).unwrap();
    let output = generate_component(&snapshot, "80:40", &CodegenOptions::default()).unwrap();
    assert!(
        output.tsx.contains("borderTop=\"solid 1px #000\""),
        "{}",
        output.tsx
    );
    assert!(
        !output
            .source_map
            .entries
            .iter()
            .any(|entry| entry.resolution == "derived-single-edge-outside-stroke")
    );
}
