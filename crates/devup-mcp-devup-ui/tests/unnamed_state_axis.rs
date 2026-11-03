use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::json;
fn fixture() -> Snapshot {
    serde_json::from_str(include_str!(
        "fixtures/dropdown-option-pure-state-axis.json"
    ))
    .unwrap()
}
fn render(s: &Snapshot) -> devup_mcp_devup_ui::codegen::CodegenOutput {
    generate_component(s, "45:2034", &CodegenOptions::default()).unwrap()
}
#[test]
fn actual_dropdown_unnamed_axis_becomes_root_hover_active_without_runtime_prop() {
    let source = fixture();
    let output = render(&source);
    assert!(output.tsx.contains("_hover={{"), "{}", output.tsx);
    assert!(output.tsx.contains("_active={{"), "{}", output.tsx);
    assert!(!output.tsx.contains("property1:"), "{}", output.tsx);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_UNNAMED_STATE_AXIS")
    );
    if let Ok(file) = std::env::var("DEVUP_DROPDOWN_STATE_PROBE") {
        std::fs::write(file, &output.tsx).unwrap();
    }
}
#[test]
fn business_name_or_missing_hover_remains_explicit_runtime_state() {
    for name in ["status", "selection"] {
        let mut s = fixture();
        rename_axis(&mut s, name);
        let o = render(&s);
        assert!(!o.tsx.contains("_hover={{"));
        assert!(o.tsx.contains(&format!("{name}:")), "{}", o.tsx);
    }
    let mut s = fixture();
    let field = axis(&s);
    s.nodes.get_mut("45:2034").unwrap().fields["componentPropertyDefinitions"][&field]["variantOptions"] =
        json!(["default", "active"]);
    s.nodes.get_mut("45:2034").unwrap().fields["childrenIds"] = json!(["45:2033", "45:2031"]);
    let o = render(&s);
    assert!(!o.tsx.contains("_active={{"));
    assert!(o.tsx.contains("property1:"));
}
#[test]
fn mixed_mobile_axis_stays_explicit_with_actionable_diagnostic() {
    let mut s = fixture();
    rename_axis(&mut s, "effect");
    s.nodes.get_mut("45:2034").unwrap().fields["componentPropertyDefinitions"]["effect"]["variantOptions"] =
        json!(["default", "hover", "active", "mobile"]);
    let o = render(&s);
    assert!(!o.tsx.contains("_hover={{"));
    assert!(o.tsx.contains("effect:"));
    assert!(
        o.diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_MIXED_STATE_AXIS")
    );
}
#[test]
fn unknown_or_descendant_changing_state_is_not_silently_inferred() {
    for case in [
        "missing-state",
        "descendant-content",
        "descendant-style",
        "topology",
    ] {
        let mut s = fixture();
        let children = s.nodes["45:2032"].fields["childrenIds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        match case {
            "missing-state" => {
                s.nodes.get_mut("45:2034").unwrap().fields["childrenIds"] =
                    json!(["45:2033", "45:2031"]);
            }
            "descendant-content" => {
                s.nodes
                    .get_mut(&children[0])
                    .unwrap()
                    .fields
                    .insert("characters".into(), json!("different"));
                s.nodes
                    .get_mut(&children[0])
                    .unwrap()
                    .fields
                    .remove("styledTextSegments");
            }
            "descendant-style" => {
                s.nodes
                    .get_mut(&children[0])
                    .unwrap()
                    .fields
                    .insert("fontSize".into(), json!(27));
                s.nodes
                    .get_mut(&children[0])
                    .unwrap()
                    .fields
                    .remove("styledTextSegments");
            }
            _ => {
                s.nodes
                    .get_mut("45:2032")
                    .unwrap()
                    .fields
                    .insert("childrenIds".into(), json!([]));
            }
        }
        let o = render(&s);
        assert!(!o.tsx.contains("_hover={{"), "{case}: {}", o.tsx);
        assert!(o.tsx.contains("property1:"), "{case}: {}", o.tsx);
    }
}
fn axis(s: &Snapshot) -> String {
    s.nodes["45:2034"].fields["componentPropertyDefinitions"]
        .as_object()
        .unwrap()
        .keys()
        .find(|k| {
            s.nodes["45:2034"].fields["componentPropertyDefinitions"][*k]["type"] == "VARIANT"
        })
        .unwrap()
        .clone()
}
fn rename_axis(s: &mut Snapshot, name: &str) {
    let key = axis(s);
    let value = s.nodes.get_mut("45:2034").unwrap().fields["componentPropertyDefinitions"]
        .as_object_mut()
        .unwrap()
        .remove(&key)
        .unwrap();
    s.nodes.get_mut("45:2034").unwrap().fields["componentPropertyDefinitions"][name] = value;
    for id in ["45:2033", "45:2032", "45:2031"] {
        let value = s.nodes.get_mut(id).unwrap().fields["variantProperties"]
            .as_object_mut()
            .unwrap()
            .remove(&key)
            .unwrap();
        s.nodes.get_mut(id).unwrap().fields["variantProperties"][name] = value;
    }
}

#[test]
fn actual_list_filter_mobile_layout_remains_a_runtime_variant() {
    let source: Snapshot =
        serde_json::from_str(include_str!("fixtures/list-filter-mixed-state-axis.json")).unwrap();
    let output = generate_component(&source, "73:6571", &CodegenOptions::default()).unwrap();
    assert!(output.tsx.contains("effect:"));
    assert!(output.tsx.contains("mobile"));
    assert!(!output.tsx.contains("_hover={{"));
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_MIXED_STATE_AXIS")
    );
}
#[test]
fn existing_explicit_effect_axis_keeps_its_selectors() {
    let mut source = fixture();
    rename_axis(&mut source, "effect");
    let output = render(&source);
    assert!(output.tsx.contains("_hover={{") && output.tsx.contains("_active={{"));
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_UNNAMED_STATE_AXIS")
    );
}
#[test]
fn multiple_generic_candidates_are_not_resolved_by_guessing() {
    let mut source = fixture();
    let key = axis(&source);
    let definition = source.nodes["45:2034"].fields["componentPropertyDefinitions"][&key].clone();
    source.nodes.get_mut("45:2034").unwrap().fields["componentPropertyDefinitions"]["Property 2"] =
        definition;
    for id in ["45:2033", "45:2032", "45:2031"] {
        let value = source.nodes[id].fields["variantProperties"][&key].clone();
        source.nodes.get_mut(id).unwrap().fields["variantProperties"]["Property 2"] = value;
    }
    let output = render(&source);
    assert!(!output.tsx.contains("_hover={{"));
    assert!(output.tsx.contains("property1:") && output.tsx.contains("Property2:"));
}
