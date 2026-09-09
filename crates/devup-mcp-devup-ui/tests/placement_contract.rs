use devup_mcp_devup_ui::codegen::{CodegenOptions, RootLayout, generate_component};
use devup_mcp_figma::{Snapshot, SnapshotChunk, merge_chunks};
use serde_json::{Value, json};

// Geometry copied verbatim from WQUW-120's 0.4.3 response diagnostic.
// This is a placement-only reconstruction, not the original 57-node snapshot.
fn snapshot() -> Snapshot {
    let evidence: Value =
        serde_json::from_str(include_str!("fixtures/wquw-120-placement-evidence.json")).unwrap();
    let mut nodes = Vec::new();
    for (source, children) in [
        (&evidence["parent"], json!(["3997:46621"])),
        (&evidence, json!(["3997:46624"])),
        (&evidence["children"][0], json!([])),
    ] {
        let mut fields = source.clone();
        fields["childrenIds"] = children;
        fields["name"] = json!("Placement");
        nodes.push(json!({"id":source["nodeId"],"type":source["nodeType"],"fields":fields,"extra":{},"fieldErrors":{}}));
    }
    nodes[0]["fields"]["parentType"] = json!("SECTION");
    let chunk: SnapshotChunk = serde_json::from_value(
        json!({"fileKey":"r4-evidence","rootIds":["3997:46582"],"nodes":nodes,"diagnostics":[]}),
    )
    .unwrap();
    merge_chunks(vec![chunk]).unwrap()
}

fn root_tag(tsx: &str) -> &str {
    let body = tsx.split("return (").nth(1).unwrap();
    body.split('>').next().unwrap()
}

#[test]
fn r4_wquw_120_root_owns_absolute_modal_coordinates() {
    for inline_instances in [true, false] {
        let output = generate_component(
            &snapshot(),
            "3997:46582",
            &CodegenOptions {
                inline_instances,
                ..Default::default()
            },
        )
        .unwrap();
        let tag = root_tag(&output.tsx);
        for prop in ["<VStack", "pos=\"relative\"", "w=\"360px\"", "h=\"740px\""] {
            assert!(tag.contains(prop), "missing {prop}: {}", output.tsx);
        }
        assert!(output.tsx.contains("left=\"50%\""));
        assert!(output.tsx.contains("py=\"232.5px\""));
    }
}

#[test]
fn r4_capture_boundary_states_root_basis_and_missing_parent() {
    let output = generate_component(&snapshot(), "3997:46582", &CodegenOptions::default()).unwrap();
    let contract = output
        .diagnostics
        .iter()
        .find(|d| d.code == "DEVUP_CODEGEN_PLACEMENT_CONTRACT")
        .expect("placement contract");
    let details = contract.details.as_ref().unwrap();
    assert_eq!(details["parentCollected"], false);
    assert_eq!(details["containingBlock"], "generated-root");
    assert_eq!(details["hostRequirements"], json!([]));
}

#[test]
fn r4_embedded_root_anchors_children_and_discloses_host_size() {
    let output = generate_component(
        &snapshot(),
        "3997:46582",
        &CodegenOptions {
            root_layout: RootLayout::Embedded,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        root_tag(&output.tsx).contains("pos=\"relative\""),
        "{}",
        output.tsx
    );
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
    assert_eq!(
        contract.fidelity_impact(),
        devup_mcp_figma::FidelityImpact::Approximated
    );
}

#[test]
fn r4_absolute_capture_root_discloses_external_containing_block() {
    let mut snapshot = snapshot();
    snapshot.nodes.remove("3997:46582");
    snapshot.roots = vec!["3997:46621".into()];
    let output = generate_component(&snapshot, "3997:46621", &CodegenOptions::default()).unwrap();
    let contract = output
        .diagnostics
        .iter()
        .find(|d| d.code == "DEVUP_CODEGEN_PLACEMENT_CONTRACT")
        .unwrap();
    assert_eq!(
        contract.details.as_ref().unwrap()["containingBlock"],
        "external-host"
    );
    assert_eq!(
        contract.fidelity_impact(),
        devup_mcp_figma::FidelityImpact::Approximated
    );
}

#[test]
fn r4_hidden_absolute_child_does_not_pin_a_fluid_screen() {
    let mut snapshot = snapshot();
    snapshot
        .nodes
        .get_mut("3997:46621")
        .unwrap()
        .fields
        .insert("visible".into(), json!(false));
    let output = generate_component(&snapshot, "3997:46582", &CodegenOptions::default()).unwrap();
    let tag = root_tag(&output.tsx);
    assert!(!tag.contains("pos=\"relative\""), "{tag}");
    assert!(!tag.contains("w=\"360px\""), "{tag}");
}

#[test]
fn r4_nonfixed_containing_block_discloses_content_size_dependency() {
    let mut snapshot = snapshot();
    snapshot
        .nodes
        .get_mut("3997:46582")
        .unwrap()
        .fields
        .insert("layoutSizingVertical".into(), json!("HUG"));
    let output = generate_component(&snapshot, "3997:46582", &CodegenOptions::default()).unwrap();
    assert!(root_tag(&output.tsx).contains("pos=\"relative\""));
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
fn r4_page_root_keeps_a_background_absolute_child_behind_content() {
    let mut snapshot = snapshot();
    snapshot
        .nodes
        .get_mut("3997:46582")
        .unwrap()
        .fields
        .insert("childrenIds".into(), json!(["3997:46621", "content"]));
    snapshot.nodes.insert("content".into(), serde_json::from_value(json!({"id":"content", "type":"TEXT", "fields":{"parentId":"3997:46582", "characters":"Foreground", "layoutPositioning":"AUTO"}})).unwrap());
    let output = generate_component(
        &snapshot,
        "3997:46582",
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        root_tag(&output.tsx).contains("zIndex=\"0\""),
        "{}",
        output.tsx
    );
    assert!(output.tsx.contains("zIndex=\"-1\""), "{}", output.tsx);
}

#[test]
fn r4_absolute_instance_wrapper_discloses_omitted_fixed_sizes() {
    let mut snapshot = snapshot();
    let node = snapshot.nodes.get_mut("3997:46582").unwrap();
    node.node_type = "INSTANCE".into();
    node.fields
        .insert("layoutPositioning".into(), json!("ABSOLUTE"));
    let output = generate_component(&snapshot, "3997:46582", &CodegenOptions::default()).unwrap();
    let contract = output
        .diagnostics
        .iter()
        .find(|d| d.code == "DEVUP_CODEGEN_PLACEMENT_CONTRACT")
        .unwrap();
    let requirements = contract.details.as_ref().unwrap()["hostRequirements"]
        .as_array()
        .unwrap();
    assert!(
        requirements
            .iter()
            .any(|r| r.as_str().unwrap().contains("root's width")),
        "{requirements:?}"
    );
    assert!(
        requirements
            .iter()
            .any(|r| r.as_str().unwrap().contains("root's height")),
        "{requirements:?}"
    );
}

#[test]
fn r4_section_selection_reports_the_actual_rendered_root() {
    let mut snapshot = snapshot();
    snapshot.nodes.insert(
        "section".into(),
        serde_json::from_value(
            json!({"id":"section", "type":"SECTION", "fields":{"childrenIds":["3997:46582"]}}),
        )
        .unwrap(),
    );
    let output = generate_component(&snapshot, "section", &CodegenOptions::default()).unwrap();
    let contract = output
        .diagnostics
        .iter()
        .find(|d| d.code == "DEVUP_CODEGEN_PLACEMENT_CONTRACT")
        .expect("actual render root contract");
    assert_eq!(contract.node_id.as_deref(), Some("3997:46582"));
}

#[test]
fn r4_absolute_root_isolates_its_background_child() {
    let mut snapshot = snapshot();
    let root = snapshot.nodes.get_mut("3997:46582").unwrap();
    root.fields
        .insert("layoutPositioning".into(), json!("ABSOLUTE"));
    root.fields
        .insert("childrenIds".into(), json!(["3997:46621", "content"]));
    snapshot.nodes.insert("content".into(), serde_json::from_value(json!({"id":"content", "type":"TEXT", "fields":{"parentId":"3997:46582", "characters":"Foreground", "layoutPositioning":"AUTO"}})).unwrap());
    let output = generate_component(
        &snapshot,
        "3997:46582",
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    let tag = root_tag(&output.tsx);
    assert!(tag.contains("pos=\"absolute\""), "{tag}");
    assert!(tag.contains("zIndex=\"0\""), "{tag}");
    assert!(output.tsx.contains("zIndex=\"-1\""), "{}", output.tsx);
}
