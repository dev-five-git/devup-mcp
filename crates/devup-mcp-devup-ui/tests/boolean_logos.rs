use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::{Snapshot, discover_asset_manifest};

#[test]
fn loading_boolean_logos_reference_exportable_svg_without_hiding_missing_operands() {
    // Captured 2026-09-08 from the two Loading screens in section 4279:7810.
    // Keep only the logo wrappers and unions; their twelve operands were absent.
    let snapshot: Snapshot =
        serde_json::from_str(include_str!("fixtures/loading-boolean-logos.json")).unwrap();
    assert_eq!(snapshot.audit().missing_children.len(), 12);
    let manifest = discover_asset_manifest(&snapshot);
    assert_eq!(manifest.assets.len(), 2, "both logos must be exportable");
    for (root, expected_asset) in [
        ("3831:10710", "3831:10710:node"),
        ("3831:10725", "3831:10725:node"),
    ] {
        let asset = manifest
            .assets
            .iter()
            .find(|a| a.asset_id == expected_asset)
            .unwrap();
        assert_eq!(asset.source_kind, "vector-node");
        assert_eq!(asset.field, "node");
        let output = generate_component(&snapshot, root, &CodegenOptions::default()).unwrap();
        assert!(output.tsx.contains(".svg"), "{}", output.tsx);
        assert!(
            output
                .source_map
                .entries
                .iter()
                .any(|e| e.asset_id.as_deref() == Some(expected_asset)
                    && e.generated_range.is_some())
        );
        assert_eq!(output.fidelity_report.assets.total, 1);
        assert_eq!(output.fidelity_report.assets.covered, 1);
    }
    assert_eq!(snapshot.audit().missing_children.len(), 12);
}

#[test]
fn an_unrepresented_missing_child_is_lossy_even_when_every_collected_node_has_tsx() {
    let mut snapshot: Snapshot =
        serde_json::from_str(include_str!("fixtures/loading-boolean-logos.json")).unwrap();
    let union = snapshot.nodes.get_mut("3831:10711").unwrap();
    union.node_type = "GROUP".into();
    let output = generate_component(&snapshot, "3831:10710", &CodegenOptions::default()).unwrap();
    assert!(output.fidelity_report.impacts.lossy > 0);
    assert_eq!(output.fidelity_report.nodes.total, 8);
    assert_eq!(output.fidelity_report.nodes.covered, 2);
    assert!(!output.fidelity_report.strict_compatible());
}

#[test]
fn missing_descendants_of_a_hidden_logo_do_not_claim_visual_loss() {
    let mut snapshot: Snapshot =
        serde_json::from_str(include_str!("fixtures/loading-boolean-logos.json")).unwrap();
    snapshot
        .nodes
        .get_mut("3831:10710")
        .unwrap()
        .fields
        .insert("visible".into(), serde_json::json!(false));
    snapshot.nodes.get_mut("3831:10711").unwrap().node_type = "GROUP".into();
    let output = generate_component(&snapshot, "3831:10710", &CodegenOptions::default()).unwrap();
    assert_eq!(output.fidelity_report.impacts.lossy, 0);
    assert!(output.fidelity_report.nodes.complete());
}
