use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::json;

fn scene(mode: &str, cross: Option<f64>) -> Snapshot {
    let mut nodes = serde_json::Map::new();
    let children = (0..9).map(|i| format!("card-{i}")).collect::<Vec<_>>();
    let mut root = json!({"id":"root","type":"FRAME","fields":{
        "name":"Captured cards","width":1280,"height":993,"layoutMode":mode,
        "layoutWrap":"WRAP","layoutSizingHorizontal":"FILL","layoutSizingVertical":"HUG",
        "itemSpacing":32,"childrenIds":children,
        "inferredAutoLayout":{"layoutMode":mode,"itemSpacing":32}
    }});
    if let Some(cross) = cross {
        root["fields"]["counterAxisSpacing"] = json!(cross);
    }
    nodes.insert("root".into(), root);
    for id in children {
        nodes.insert(
            id.clone(),
            json!({"id":id,"type":"FRAME","fields":{
                "name":"Card","parentId":"root","width":405,"height":299,
                "layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED"
            }}),
        );
    }
    serde_json::from_value(
        json!({"fileKey":"captured-wrap","roots":["root"],"nodes":nodes,"diagnostics":[]}),
    )
    .unwrap()
}

#[test]
fn captured_horizontal_wrap_keeps_nine_fixed_cards_and_independent_gaps() {
    let output = generate_component(
        &scene("HORIZONTAL", Some(48.0)),
        "root",
        &CodegenOptions::default(),
    )
    .unwrap();
    assert!(
        !devup_mcp_devup_ui::provenance::validate_fidelity(
            &scene("HORIZONTAL", Some(48.0)),
            "root",
            &output
        )
        .unwrap()
        .uncovered_layout
        .contains(&"root#itemSpacing".to_owned())
    );
    assert!(
        output
            .source_map
            .entries
            .iter()
            .any(|entry| entry.node_id.as_deref() == Some("root")
                && entry.property.as_deref() == Some("itemSpacing")
                && entry.generated_property.as_deref() == Some("columnGap=\"32px\""))
    );
    for expected in ["flexWrap=\"wrap\"", "columnGap=\"32px\"", "rowGap=\"48px\""] {
        assert!(
            output.tsx.contains(expected),
            "missing {expected}: {}",
            output.tsx
        );
    }
    assert!(!output.tsx.contains("gridTemplate"), "{}", output.tsx);
    assert_eq!(output.tsx.matches("w=\"405px\"").count(), 9);
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_PROPERTY_UNMAPPED"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn wrap_gap_contract_rejects_wrong_main_gap_value_and_cross_axis_evidence() {
    for mode in ["HORIZONTAL", "VERTICAL"] {
        let snapshot = scene(mode, Some(48.0));
        let output = generate_component(&snapshot, "root", &CodegenOptions::default()).unwrap();
        assert!(
            !devup_mcp_devup_ui::provenance::validate_fidelity(&snapshot, "root", &output)
                .unwrap()
                .uncovered_layout
                .contains(&"root#itemSpacing".to_owned())
        );
        let mut changed = output.clone();
        let main = if mode == "HORIZONTAL" {
            "columnGap"
        } else {
            "rowGap"
        };
        changed.tsx = changed
            .tsx
            .replace(&format!("{main}=\"32px\""), &format!("{main}=\"33px\""));
        let fidelity =
            devup_mcp_devup_ui::provenance::validate_fidelity(&snapshot, "root", &changed).unwrap();
        assert!(
            fidelity
                .uncovered_layout
                .contains(&"root#itemSpacing".to_owned())
        );
        let mut wrong_axis = output.clone();
        let cross = wrong_axis
            .source_map
            .entries
            .iter()
            .find(|e| {
                e.node_id.as_deref() == Some("root")
                    && e.property.as_deref() == Some("counterAxisSpacing")
            })
            .unwrap()
            .generated_range
            .clone();
        wrong_axis
            .source_map
            .entries
            .iter_mut()
            .filter(|e| {
                e.node_id.as_deref() == Some("root") && e.property.as_deref() == Some("itemSpacing")
            })
            .for_each(|entry| entry.generated_range = cross.clone());
        let fidelity =
            devup_mcp_devup_ui::provenance::validate_fidelity(&snapshot, "root", &wrong_axis)
                .unwrap();
        assert!(
            fidelity
                .uncovered_layout
                .contains(&"root#itemSpacing".to_owned())
        );
    }
}

#[test]
fn vertical_wrap_exchanges_main_and_cross_gap_axes() {
    let output = generate_component(
        &scene("VERTICAL", Some(12.0)),
        "root",
        &CodegenOptions::default(),
    )
    .unwrap();
    for expected in ["flexWrap=\"wrap\"", "rowGap=\"32px\"", "columnGap=\"12px\""] {
        assert!(
            output.tsx.contains(expected),
            "missing {expected}: {}",
            output.tsx
        );
    }
}

#[test]
fn absent_cross_gap_is_not_invented() {
    let wrapped = generate_component(
        &scene("HORIZONTAL", None),
        "root",
        &CodegenOptions::default(),
    )
    .unwrap();
    assert!(wrapped.tsx.contains("flexWrap=\"wrap\""), "{}", wrapped.tsx);
    assert!(wrapped.tsx.contains("gap=\"32px\""), "{}", wrapped.tsx);
    assert!(!wrapped.tsx.contains("rowGap="));
}

#[test]
fn wrapped_space_between_keeps_zero_cross_gap_and_distribution() {
    let mut snapshot = scene("HORIZONTAL", Some(0.0));
    snapshot
        .nodes
        .get_mut("root")
        .unwrap()
        .fields
        .insert("counterAxisAlignContent".into(), json!("SPACE_BETWEEN"));
    let output = generate_component(&snapshot, "root", &CodegenOptions::default()).unwrap();
    for expected in [
        "flexWrap=\"wrap\"",
        "columnGap=\"32px\"",
        "rowGap=\"0px\"",
        "alignContent=\"space-between\"",
    ] {
        assert!(
            output.tsx.contains(expected),
            "missing {expected}: {}",
            output.tsx
        );
    }
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_PROPERTY_UNMAPPED"),
        "{:?}",
        output.diagnostics
    );
}
