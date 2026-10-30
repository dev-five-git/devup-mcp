use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_devup_ui::provenance::ProjectionDisposition;
use devup_mcp_figma::Snapshot;

fn assert_component_text_traces(raw: &str, excluded: &[&str]) {
    let snapshot: Snapshot = serde_json::from_str(raw).unwrap();
    let output = generate_component(&snapshot, &snapshot.roots[0], &CodegenOptions::default())
        .expect("componentTsx must account for every source node");
    println!("{}\n{}", snapshot.roots[0], output.tsx);

    let (signature, body) = output.tsx.split_once("return (").unwrap();
    for node in snapshot
        .nodes
        .values()
        .filter(|node| node.node_type == "TEXT")
    {
        assert_eq!(
            output
                .projection_trace
                .entries
                .iter()
                .filter(|entry| entry.node_id == node.id)
                .count(),
            1,
            "TEXT {} must be traced exactly once",
            node.id
        );
        let characters = node.typed_view().string("characters").unwrap();
        let default = serde_json::to_string(characters).unwrap();
        assert!(
            signature.contains(&format!("= {default}")),
            "TEXT {} needs a prop default: {}",
            node.id,
            output.tsx
        );
        assert!(
            !body.contains(characters),
            "TEXT {} sample copy must not be a JSX child: {}",
            node.id,
            output.tsx
        );
    }
    for node_id in excluded {
        let entry = output
            .projection_trace
            .entries
            .iter()
            .find(|entry| entry.node_id == *node_id)
            .unwrap();
        assert_eq!(entry.disposition, ProjectionDisposition::Ignored);
        assert_eq!(entry.reason, "unrepresented-variant-child");
        assert!(entry.generated_range.is_none());
        assert!(
            !output
                .source_map
                .entries
                .iter()
                .any(|entry| entry.node_id.as_deref() == Some(node_id))
        );
        assert!(output.diagnostics.iter().any(|diagnostic| {
            diagnostic.node_id.as_deref() == Some(node_id)
                && diagnostic.code == "DEVUP_CODEGEN_VARIANT_CHILD_FALLBACK"
                && diagnostic.fidelity_impact() == devup_mcp_figma::FidelityImpact::Lossy
        }));
    }
    assert!(!output.fidelity_report.strict_compatible());
}

#[test]
fn applications_card_component_text_traces() {
    assert_component_text_traces(
        include_str!("fixtures/chondrogen_applications_card_set.json"),
        &["81:12384"],
    );
}

#[test]
fn rnd_link_row_component_text_traces() {
    assert_component_text_traces(
        include_str!("fixtures/chondrogen_rnd_link_row_set.json"),
        &["51:5764"],
    );
}

#[test]
fn tech_step_component_text_traces() {
    assert_component_text_traces(
        include_str!("fixtures/chondrogen_tech_step_set.json"),
        &["51:5505", "51:5506"],
    );
}
