use devup_mcp_devup_ui::codegen::{CodegenOptions, CodegenOutput, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::json;
fn fixture() -> Snapshot {
    serde_json::from_str(include_str!("fixtures/home-news-mixed-strokes.json")).unwrap()
}
fn generate(s: &Snapshot) -> CodegenOutput {
    generate_component(
        s,
        "desktop",
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap()
}
fn opening(output: &CodegenOutput, id: &str) -> String {
    let entry = output
        .source_map
        .entries
        .iter()
        .find(|e| e.node_id.as_deref() == Some(id) && e.property.is_none())
        .unwrap();
    let range = entry.generated_range.as_ref().unwrap();
    output.tsx[range.start..range.end]
        .split('>')
        .next()
        .unwrap()
        .into()
}
#[test]
fn actual_home_bottom_edge_is_absorbed_by_captured_padding() {
    let s = fixture();
    assert_eq!(s.nodes["51:5838"].fields["strokesIncludedInLayout"], false);
    let output = generate(&s);
    let row = opening(&output, "51:5838");
    assert!(row.contains("pt=\"24px\""), "{row}");
    assert!(row.contains("pb=\"23px\""), "{row}");
    assert!(row.contains("borderBottom="), "{row}");
    assert!(!row.contains("h="), "source remains HUG");
    assert!(
        output
            .source_map
            .entries
            .iter()
            .any(|e| e.node_id.as_deref() == Some("51:5838")
                && e.property.as_deref() == Some("paddingBottom")
                && e.generated_property.as_deref() == Some("pb=\"23px\"")),
        "{:?}",
        output.source_map.entries
    );
}
#[test]
fn true_missing_unknown_error_and_insufficient_cases_are_not_compensated() {
    for case in [
        "true",
        "missing",
        "align",
        "align-error",
        "weight-error",
        "weight-missing",
        "insufficient",
        "dash",
        "gradient",
    ] {
        let mut s = fixture();
        let node = s.nodes.get_mut("51:5838").unwrap();
        match case {
            "true" => {
                node.fields
                    .insert("strokesIncludedInLayout".into(), json!(true));
            }
            "missing" => {
                node.fields.remove("strokesIncludedInLayout");
            }
            "align" => {
                node.fields
                    .insert("strokeAlign".into(), json!({"$unsupported":"symbol"}));
            }
            "align-error" => {
                node.field_errors
                    .insert("strokeAlign".into(), "read failed".into());
            }
            "weight-error" => {
                node.field_errors
                    .insert("strokeBottomWeight".into(), "read failed".into());
            }
            "weight-missing" => {
                node.fields.remove("strokeTopWeight");
            }
            "insufficient" => {
                node.fields.insert("paddingBottom".into(), json!(0));
                node.fields.get_mut("inferredAutoLayout").unwrap()["paddingBottom"] = json!(0);
            }
            "dash" => {
                node.fields.insert("dashPattern".into(), json!([2, 2]));
            }
            _ => {
                node.fields.insert(
                    "strokes".into(),
                    json!([{"type":"GRADIENT_LINEAR","visible":true}]),
                );
            }
        }
        let output = generate(&s);
        let row = opening(&output, "51:5838");
        assert!(!row.contains("pb=\"23px\""), "{case}:{row}");
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|d| d.node_id.as_deref() == Some("51:5838")
                    && d.details.as_ref().is_some_and(|v| v["calculation"]
                        .as_str()
                        .is_some_and(|x| x.starts_with("Captured inside per-edge")))),
            "{case}"
        );
    }
}
#[test]
fn existing_effect_shadow_and_border_paint_are_preserved() {
    let mut s = fixture();
    s.nodes.get_mut("51:5838").unwrap().fields.insert("effects".into(),json!([{"type":"DROP_SHADOW","visible":true,"radius":4,"spread":0,"offset":{"x":0,"y":2},"color":{"r":0,"g":0,"b":0,"a":0.5}}]));
    let output = generate(&s);
    let row = opening(&output, "51:5838");
    assert!(row.contains("boxShadow="), "{row}");
    assert!(row.contains("borderBottom="), "{row}");
    assert!(row.contains("pb=\"23px\""), "{row}");
}
