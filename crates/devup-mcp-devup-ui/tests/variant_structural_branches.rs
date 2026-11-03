use devup_mcp_devup_ui::codegen::{CodegenOptions, CodegenOutput, generate_component};
use devup_mcp_figma::{FidelityImpact, Snapshot};
use serde_json::json;

fn generate(source: &str) -> CodegenOutput {
    let snapshot: Snapshot = serde_json::from_str(source).unwrap();
    generate_component(&snapshot, &snapshot.roots[0], &CodegenOptions::default()).unwrap()
}

fn emitted(output: &CodegenOutput, id: &str) -> bool {
    output.source_map.entries.iter().any(|entry| {
        entry.node_id.as_deref() == Some(id)
            && entry.property.is_none()
            && entry.resolution == "node"
    })
}

#[test]
fn actual_footer_keeps_mobile_column_and_both_visible_logo_instances() {
    let output = generate(include_str!("fixtures/chondrogen_footer_set.json"));
    assert!(!output.tsx.contains("{/*"), "{}", output.tsx);
    for id in ["30:1119", "43:1187", "41:901", "43:1189"] {
        assert!(
            emitted(&output, id),
            "missing captured node {id}: {}",
            output.tsx
        );
    }
    assert_eq!(output.tsx.matches("as=\"footer\"").count(), 2);
    assert!(
        output.tsx.contains("[\"none\",null,null,null,\"flex\"]"),
        "{}",
        output.tsx
    );
    assert!(
        output.tsx.contains("[\"flex\",null,null,null,\"none\"]"),
        "{}",
        output.tsx
    );
    let mobile = output
        .source_map
        .entries
        .iter()
        .find(|e| e.node_id.as_deref() == Some("43:1188") && e.resolution == "node")
        .unwrap()
        .generated_range
        .as_ref()
        .unwrap();
    assert!(output.tsx[mobile.start..mobile.end].contains("<VStack"));
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_VARIANT_CHILD_FALLBACK")
    );
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_PROPERTY_UNMAPPED"
                && d.property.as_deref() == Some("display")),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn actual_header_preserves_each_captured_topology_and_runtime_type_guard() {
    let output = generate(include_str!("fixtures/chondrogen_header_set.json"));
    for id in [
        "23:1576", "44:5181", "40:265", "43:1099", "44:5193", "43:1087",
    ] {
        assert!(emitted(&output, id), "missing captured Header variant {id}");
    }
    assert!(output.tsx.contains("type === \"scroll\""));
    assert!(output.tsx.contains("type === \"pageTop\""));
    assert!(output.tsx.contains("type === \"default\""));
    assert_eq!(output.tsx.matches("as=\"header\"").count(), 6);
    assert!(!output.tsx.contains("alt={["));
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_VARIANT_CHILD_FALLBACK")
    );
}

#[test]
fn runtime_prop_variants_keep_extra_captured_children_without_viewport_wrappers() {
    let snapshot = json!({"fileKey":"branches","roots":["set"],"diagnostics":[],"nodes":{
        "set":{"id":"set","type":"COMPONENT_SET","fields":{"name":"Card","childrenIds":["simple","expanded"],"componentPropertyDefinitions":{"kind":{"type":"VARIANT","defaultValue":"simple","variantOptions":["simple","expanded"]}}}},
        "simple":{"id":"simple","type":"COMPONENT","fields":{"name":"kind=simple","variantProperties":{"kind":"simple"},"childrenIds":["simple-text"],"layoutMode":"VERTICAL"}},
        "expanded":{"id":"expanded","type":"COMPONENT","fields":{"name":"kind=expanded","variantProperties":{"kind":"expanded"},"childrenIds":["expanded-text","extra"],"layoutMode":"HORIZONTAL"}},
        "simple-text":{"id":"simple-text","type":"TEXT","fields":{"name":"Title","characters":"Simple captured title","fontSize":20}},
        "expanded-text":{"id":"expanded-text","type":"TEXT","fields":{"name":"Title","characters":"Expanded captured title","fontSize":20}},
        "extra":{"id":"extra","type":"TEXT","fields":{"name":"Extra","characters":"Additional content","fontSize":16}}
    }});
    let output = generate(&snapshot.to_string());
    for id in [
        "simple",
        "expanded",
        "simple-text",
        "expanded-text",
        "extra",
    ] {
        assert!(emitted(&output, id), "missing {id}");
    }
    for value in [
        "Simple captured title",
        "Expanded captured title",
        "Additional content",
        "kind === \"simple\"",
        "kind === \"expanded\"",
    ] {
        assert!(output.tsx.contains(value), "missing {value}");
    }
    assert!(
        !output.tsx.contains("<Box"),
        "branching must not add a layout wrapper: {}",
        output.tsx
    );
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.fidelity_impact == Some(FidelityImpact::Lossy)),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn hover_topology_remains_an_explicit_blocker_instead_of_claiming_preservation() {
    let snapshot = json!({"fileKey":"effect-branches","roots":["set"],"diagnostics":[],"nodes":{
        "set":{"id":"set","type":"COMPONENT_SET","fields":{"name":"Button","childrenIds":["default","hover"],"componentPropertyDefinitions":{"effect":{"type":"VARIANT","defaultValue":"default","variantOptions":["default","hover"]}}}},
        "default":{"id":"default","type":"COMPONENT","fields":{"name":"effect=default","variantProperties":{"effect":"default"},"childrenIds":["label"],"layoutMode":"HORIZONTAL"}},
        "hover":{"id":"hover","type":"COMPONENT","fields":{"name":"effect=hover","variantProperties":{"effect":"hover"},"childrenIds":["hover-label","hover-extra"],"layoutMode":"HORIZONTAL"}},
        "label":{"id":"label","type":"TEXT","fields":{"name":"Label","characters":"Submit","fontSize":16}},
        "hover-label":{"id":"hover-label","type":"TEXT","fields":{"name":"Label","characters":"Submit","fontSize":16}},
        "hover-extra":{"id":"hover-extra","type":"TEXT","fields":{"name":"Extra","characters":"Extra hover content","fontSize":16}}
    }});
    let output = generate(&snapshot.to_string());
    let issue = output
        .diagnostics
        .iter()
        .find(|d| d.code == "DEVUP_CODEGEN_EFFECT_TOPOLOGY_UNSUPPORTED")
        .expect("changing effect topology requires an explicit interaction owner");
    assert_eq!(issue.fidelity_impact, Some(FidelityImpact::Lossy));
}
