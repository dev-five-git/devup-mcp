use devup_mcp_devup_ui::codegen::{CodegenOptions, CodegenOutput, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::json;

fn generate(mode: &str, distribution: &str, children: &[(&str, bool)]) -> CodegenOutput {
    let mut snapshot: Snapshot = serde_json::from_value(json!({
        "fileKey":"alignment", "roots":["root"], "diagnostics":[], "nodes": {
            "root":{"id":"root","type":"FRAME","fields":{
                "layoutMode":mode,"inferredAutoLayout":{"layoutMode":mode},
                "primaryAxisAlignItems":distribution,"counterAxisAlignItems":"CENTER",
                "width":40,"height":40,"layoutSizingHorizontal":"FIXED",
                "layoutSizingVertical":"FIXED","childrenIds":[]
            }}
        }
    }))
    .unwrap();
    let ids: Vec<_> = (0..children.len()).map(|i| format!("child-{i}")).collect();
    snapshot
        .nodes
        .get_mut("root")
        .unwrap()
        .fields
        .insert("childrenIds".into(), json!(ids));
    for (id, (position, visible)) in ids.iter().zip(children) {
        snapshot.nodes.insert(id.clone(), serde_json::from_value(json!({
            "id":id,"type":"TEXT","fields":{
                "characters":"x",
                "parentId":"root","layoutPositioning":position,"visible":visible,
                "width":8,"height":8,"layoutSizingHorizontal":"FIXED",
                "layoutSizingVertical":"FIXED","fills":[{"type":"SOLID","color":{"r":1,"g":0,"b":0}}]
            }
        })).unwrap());
    }
    generate_component(&snapshot, "root", &CodegenOptions::default()).unwrap()
}

#[test]
fn lone_in_flow_child_is_centered_on_either_primary_axis() {
    for mode in ["HORIZONTAL", "VERTICAL"] {
        for children in [
            vec![("AUTO", true)],
            vec![("AUTO", true), ("ABSOLUTE", true)],
            vec![("AUTO", false), ("AUTO", true)],
        ] {
            let output = generate(mode, "SPACE_BETWEEN", &children);
            assert!(
                output.tsx.contains("justifyContent=\"center\""),
                "{mode}: {}",
                output.tsx
            );
            assert!(
                output.tsx.contains("alignItems=\"center\""),
                "{}",
                output.tsx
            );
        }
    }
}

#[test]
fn distribution_without_exactly_one_in_flow_child_is_preserved() {
    for children in [
        vec![],
        vec![("ABSOLUTE", true)],
        vec![("AUTO", false)],
        vec![("AUTO", true), ("AUTO", true)],
    ] {
        let output = generate("HORIZONTAL", "SPACE_BETWEEN", &children);
        assert!(
            output.tsx.contains("justifyContent=\"space-between\""),
            "{}",
            output.tsx
        );
    }
    for (distribution, css) in [
        ("SPACE_AROUND", "space-around"),
        ("SPACE_EVENLY", "space-evenly"),
        ("MAX", "flex-end"),
    ] {
        let output = generate("HORIZONTAL", distribution, &[("AUTO", true)]);
        assert!(
            output.tsx.contains(&format!("justifyContent=\"{css}\"")),
            "{}",
            output.tsx
        );
    }
}

#[test]
fn centered_distribution_source_map_identifies_the_semantic_conversion() {
    let output = generate(
        "VERTICAL",
        "SPACE_BETWEEN",
        &[("AUTO", true), ("ABSOLUTE", true)],
    );
    let entry = output
        .source_map
        .entries
        .iter()
        .find(|e| {
            e.node_id.as_deref() == Some("root")
                && e.property.as_deref() == Some("primaryAxisAlignItems")
        })
        .unwrap();
    assert_eq!(entry.resolution, "derived-lone-child-center");
    let range = entry.generated_range.as_ref().unwrap();
    assert_eq!(
        &output.tsx[range.start..range.end],
        "justifyContent=\"center\""
    );
}

#[test]
fn grid_and_free_layout_do_not_receive_flex_distribution_correction() {
    for mode in ["GRID", "NONE"] {
        let output = generate(mode, "SPACE_BETWEEN", &[("AUTO", true)]);
        assert!(
            !output.tsx.contains("justifyContent=\"center\""),
            "{}",
            output.tsx
        );
        assert!(
            !output
                .source_map
                .entries
                .iter()
                .any(|e| e.resolution == "derived-lone-child-center")
        );
    }
}
