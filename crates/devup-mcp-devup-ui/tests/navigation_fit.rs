use devup_mcp_devup_ui::{ui_layout::layout_risks, ui_validate::validate_devup_ui_tsx};

#[test]
fn measured_navigation_requires_a_mobile_menu_before_it_clips() {
    let tsx = r#"export const Header = () => <Flex as="nav" gap="24px" w="100%"><Text w="160px">Company</Text><Text w="160px">Services</Text><Text w="160px">Contact</Text></Flex>;"#;
    let risks = layout_risks(tsx, Some(1440.0));
    assert!(
        risks["risks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|risk| risk["rule"] == "navigation-fit"),
        "{risks}"
    );
    let validation = validate_devup_ui_tsx(tsx, None, false);
    assert!(
        validation
            .violations
            .iter()
            .any(|violation| violation.rule == "navigation-fit")
    );
    let mobile = tsx.replace(
        "w=\"100%\"",
        "w=\"100%\" display={[\"none\",null,\"flex\"]}",
    );
    assert!(
        !layout_risks(&mobile, None)["risks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|risk| risk["rule"] == "navigation-fit")
    );
}

#[test]
fn figma_header_export_exposes_measured_fit_risk() {
    use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
    use devup_mcp_figma::Snapshot;
    let mut scene: Snapshot =
        serde_json::from_str(include_str!("fixtures/staggered-sequence.json")).unwrap();
    scene
        .nodes
        .get_mut("root")
        .unwrap()
        .fields
        .insert("name".into(), serde_json::json!("Header navigation"));
    // This is a header with simple links rather than numbered content cards.
    for id in ["text1", "text2", "text3", "text4"] {
        scene
            .nodes
            .get_mut(id)
            .unwrap()
            .fields
            .insert("characters".into(), serde_json::json!("Link"));
    }
    let out = generate_component(&scene, "root", &CodegenOptions::default()).unwrap();
    assert!(out.tsx.contains("data-navigation-width"), "{}", out.tsx);
    assert!(
        layout_risks(&out.tsx, None)["risks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|risk| risk["rule"] == "navigation-fit")
    );
}
