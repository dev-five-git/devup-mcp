use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::json;

fn source() -> Snapshot {
    serde_json::from_str(include_str!("fixtures/clipped-free-pagemenu.json")).unwrap()
}

fn render(snapshot: &Snapshot) -> String {
    generate_component(
        snapshot,
        "73:8525",
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap()
    .tsx
}

#[test]
fn inferred_layout_cannot_discard_the_captured_negative_free_frame_offset() {
    let snapshot = source();
    let output = render(&snapshot);
    if let Ok(path) = std::env::var("DEVUP_OFFSET_PROBE_OUTPUT") {
        std::fs::write(path, &output).unwrap();
    }
    assert_eq!(snapshot.nodes["73:8525"].fields["layoutMode"], "NONE");
    assert_eq!(
        snapshot.nodes["73:8525"].fields["overflowDirection"],
        "NONE"
    );
    for expected in [
        "pos=\"relative\"",
        "h=\"47px\"",
        "overflow=\"hidden\"",
        "pos=\"absolute\"",
        "left=\"clamp(-151px, calc(100% - 582px), 0px)\"",
        "top=\"0px\"",
        "w=\"582px\"",
    ] {
        assert!(output.contains(expected), "missing {expected}: {output}");
    }
    assert!(
        !output.contains("overflowX="),
        "scrolling was never captured: {output}"
    );
}

#[test]
fn actual_auto_layout_and_unclipped_inference_are_not_replaced_by_free_positioning() {
    for (field, value) in [
        ("layoutMode", json!("HORIZONTAL")),
        ("clipsContent", json!(false)),
    ] {
        let mut snapshot = source();
        snapshot
            .nodes
            .get_mut("73:8525")
            .unwrap()
            .fields
            .insert(field.into(), value);
        let output = render(&snapshot);
        assert!(!output.contains("clamp("), "{field}: {output}");
    }
}

#[test]
fn a_child_inside_the_free_frame_does_not_gain_an_absolute_offset() {
    let mut snapshot = source();
    let child = snapshot.nodes.get_mut("73:8526").unwrap();
    child.fields.insert("x".into(), json!(0));
    child.fields["absoluteBoundingBox"]["x"] = json!(38352);
    let output = render(&snapshot);
    assert!(!output.contains("pos=\"absolute\""), "{output}");
}

#[test]
fn a_fixed_parent_keeps_its_captured_offset_without_fluid_clamping() {
    let mut snapshot = source();
    snapshot
        .nodes
        .get_mut("73:8525")
        .unwrap()
        .fields
        .insert("layoutSizingHorizontal".into(), json!("FIXED"));
    let output = render(&snapshot);
    assert!(output.contains("left=\"-151px\""), "{output}");
    assert!(!output.contains("clamp("), "{output}");
}
