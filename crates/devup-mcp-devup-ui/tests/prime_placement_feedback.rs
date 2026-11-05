use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::json;

fn snapshot() -> Snapshot {
    serde_json::from_str(include_str!("fixtures/prime-placement.json")).unwrap()
}

#[test]
fn absolute_rectangle_retains_both_measured_axes_inside_a_frame() {
    let output = generate_component(
        &snapshot(),
        "card",
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(output.tsx.contains("boxSize=\"16px\""), "{}", output.tsx);
    assert!(!output.tsx.contains("h=\"100%\""), "{}", output.tsx);
}

#[test]
fn detached_render_root_retains_fill_even_when_snapshot_roots_are_different() {
    let mut snapshot = snapshot();
    snapshot.nodes.remove("row");
    let output = generate_component(
        &snapshot,
        "title",
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(output.tsx.contains("w=\"100%\""), "{}", output.tsx);
    assert!(output.tsx.contains("maxW=\"420px\""), "{}", output.tsx);
    let contract = output
        .diagnostics
        .iter()
        .find(|d| d.code == "DEVUP_CODEGEN_PLACEMENT_CONTRACT")
        .unwrap();
    assert!(
        !contract.details.as_ref().unwrap()["hostRequirements"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn component_reference_reports_the_parent_allocation_that_it_cannot_apply() {
    let output = generate_component(&snapshot(), "row", &CodegenOptions::default()).unwrap();
    let contract = output
        .diagnostics
        .iter()
        .find(|d| {
            d.code == "DEVUP_CODEGEN_INSTANCE_PLACEMENT_CONTRACT"
                && d.node_id.as_deref() == Some("title")
        })
        .expect("reference must disclose unapplied caller sizing");
    let details = contract.details.as_ref().unwrap();
    assert_eq!(details["requiredRootProps"]["flex"], "1");
    assert_eq!(details["requiredRootProps"]["maxW"], "420px");
    assert_eq!(details["sourceSize"], json!({"width":420.0,"height":94.0}));
    assert_eq!(
        contract.fidelity_impact(),
        devup_mcp_figma::FidelityImpact::Approximated
    );
}

#[test]
fn mixed_axis_detached_fill_is_preserved_without_a_maximum() {
    let mut snapshot = snapshot();
    snapshot.nodes.remove("row");
    let title = snapshot.nodes.get_mut("title").unwrap();
    title
        .fields
        .insert("maxWidth".into(), serde_json::Value::Null);
    title
        .fields
        .insert("layoutSizingVertical".into(), json!("FIXED"));
    let output = generate_component(
        &snapshot,
        "title",
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(output.tsx.contains("w=\"100%\""), "{}", output.tsx);
    assert!(output.tsx.contains("h=\"94px\""), "{}", output.tsx);
}

#[test]
fn rectangle_repair_does_not_override_fluid_constraints_or_failed_reads() {
    for (field, value, read_error) in [
        (
            "constraints",
            json!({"horizontal":"STRETCH","vertical":"MIN"}),
            false,
        ),
        ("layoutSizingHorizontal", json!("FILL"), false),
        ("width", json!(16), true),
    ] {
        let mut snapshot = snapshot();
        let marker = snapshot.nodes.get_mut("marker").unwrap();
        if read_error {
            marker
                .field_errors
                .insert(field.into(), "unavailable".into());
        } else {
            marker.fields.insert(field.into(), value);
        }
        let output = generate_component(
            &snapshot,
            "card",
            &CodegenOptions {
                inline_instances: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            !output.tsx.contains("boxSize=\"16px\""),
            "{field}: {}",
            output.tsx
        );
    }
}
