use devup_mcp_devup_ui::codegen::{CodegenOptions, CodegenOutput, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::json;

fn scene() -> Snapshot {
    serde_json::from_value(json!({
        "fileKey": "fill-overflow", "roots": ["root"], "diagnostics": [],
        "nodes": {
            "root": {"id":"root", "type":"FRAME", "fields":{
                "layoutMode":"HORIZONTAL", "layoutSizingHorizontal":"FIXED",
                "layoutSizingVertical":"HUG", "width":320, "height":40,
                "itemSpacing":40, "childrenIds":["hug","fill"]
            }},
            "hug": {"id":"hug", "type":"FRAME", "fields":{
                "parentId":"root", "layoutMode":"VERTICAL",
                "layoutSizingHorizontal":"HUG", "layoutSizingVertical":"HUG",
                "width":160, "height":40, "childrenIds":[]
            }},
            "fill": {"id":"fill", "type":"FRAME", "fields":{
                "parentId":"root", "layoutMode":"HORIZONTAL",
                "layoutSizingHorizontal":"FILL", "layoutSizingVertical":"HUG",
                "width":120, "height":40, "itemSpacing":20,
                "primaryAxisAlignItems":"MAX", "childrenIds":["a","b"]
            }},
            "a": {"id":"a", "type":"RECTANGLE", "fields":{
                "parentId":"fill", "width":70, "height":40,
                "layoutSizingHorizontal":"FIXED", "layoutSizingVertical":"FIXED"
            }},
            "b": {"id":"b", "type":"RECTANGLE", "fields":{
                "parentId":"fill", "width":70, "height":40,
                "layoutSizingHorizontal":"FIXED", "layoutSizingVertical":"FIXED"
            }}
        }
    }))
    .unwrap()
}

fn generate(snapshot: &Snapshot) -> CodegenOutput {
    let mut snapshot = snapshot.clone();
    for node in snapshot.nodes.values_mut() {
        if node.fields.contains_key("layoutMode") {
            node.fields
                .insert("inferredAutoLayout".into(), json!(node.fields));
        }
    }
    generate_component(&snapshot, "root", &CodegenOptions::default()).unwrap()
}

fn fill_tag(output: &CodegenOutput) -> &str {
    let entry = output
        .source_map
        .entries
        .iter()
        .find(|entry| entry.node_id.as_deref() == Some("fill") && entry.property.is_none())
        .unwrap();
    let range = entry.generated_range.as_ref().unwrap();
    output.tsx[range.start..range.end]
        .split('>')
        .next()
        .unwrap()
}

#[test]
fn collectively_overflowing_row_keeps_its_fill_allocation() {
    let output = generate(&scene());
    // Neither 70px child exceeds the 120px allocation; together with the
    // 20px gap they do. CSS must allow the FILL box to remain 120px wide.
    assert!(fill_tag(&output).contains("minW=\"0\""), "{}", output.tsx);
    assert!(fill_tag(&output).contains("flex=\"1\""));
    assert!(!fill_tag(&output).contains("120px"));
}

#[test]
fn derived_minimum_maps_to_fill_intent_not_an_absent_min_width() {
    let output = generate(&scene());
    let mappings: Vec<_> = output
        .source_map
        .entries
        .iter()
        .filter(|entry| {
            entry.node_id.as_deref() == Some("fill")
                && entry.generated_range.as_ref().is_some_and(|range| {
                    output.tsx.get(range.start..range.end) == Some("minW=\"0\"")
                })
        })
        .collect();
    assert!(
        mappings
            .iter()
            .any(|entry| entry.property.as_deref() == Some("layoutSizingHorizontal"))
    );
    assert!(
        !mappings
            .iter()
            .any(|entry| entry.property.as_deref() == Some("minWidth"))
    );
}

#[test]
fn fitting_wrapping_and_out_of_flow_children_do_not_force_a_reset() {
    for (field, value) in [
        ("width", json!(180)),
        ("layoutWrap", json!("WRAP")),
        ("layoutMode", json!("VERTICAL")),
    ] {
        let mut snapshot = scene();
        snapshot
            .nodes
            .get_mut("fill")
            .unwrap()
            .fields
            .insert(field.into(), value);
        assert!(!fill_tag(&generate(&snapshot)).contains("minW="), "{field}");
    }
    for (field, value) in [
        ("visible", json!(false)),
        ("layoutPositioning", json!("ABSOLUTE")),
    ] {
        let mut snapshot = scene();
        snapshot
            .nodes
            .get_mut("b")
            .unwrap()
            .fields
            .insert(field.into(), value);
        assert!(!fill_tag(&generate(&snapshot)).contains("minW="), "{field}");
    }
}

#[test]
fn an_explicit_minimum_is_preserved() {
    let mut snapshot = scene();
    snapshot
        .nodes
        .get_mut("fill")
        .unwrap()
        .fields
        .insert("minWidth".into(), json!(100));
    assert!(fill_tag(&generate(&snapshot)).contains("minW=\"100px\""));
}
