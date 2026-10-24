use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;

fn generate(raw: &str) -> String {
    let snapshot: Snapshot = serde_json::from_str(raw).unwrap();
    let mut options = CodegenOptions {
        asset_names_per_node: true,
        ..CodegenOptions::default()
    };
    for node in snapshot
        .nodes
        .values()
        .filter(|node| node.node_type == "TEXT")
    {
        if let Some(style) = node
            .typed_view()
            .string("textStyleId")
            .filter(|id| !id.is_empty())
        {
            options
                .text_style_tokens
                .insert(style.into(), "bodySm".into());
        }
    }
    generate_component(&snapshot, &snapshot.roots[0], &options)
        .unwrap()
        .tsx
}

#[test]
fn responsive_typography_remains_a_scalar_token() {
    let tsx = generate(include_str!("fixtures/chondrogen_board_row_set.json"));
    assert!(!tsx.contains("typography={["), "{tsx}");
    assert!(tsx.contains("typography=\"bodySm\""), "{tsx}");
}

#[test]
fn responsive_sources_remain_scalar_images() {
    let tsx = generate(include_str!("fixtures/chondrogen_cta_banner_set.json"));
    assert!(!tsx.contains("src={["), "{tsx}");
    assert!(
        tsx.contains("caret-2.svg") && tsx.contains("caret.svg"),
        "{tsx}"
    );
}

#[test]
fn absent_responsive_variant_values_have_explicit_undefined_branches() {
    let tsx = generate(include_str!("fixtures/chondrogen_page_title_set.json"));
    assert!(tsx.contains("White: undefined"), "{tsx}");
}

#[test]
fn referenced_boolean_properties_are_declared_with_text_properties() {
    let tsx = generate(include_str!("fixtures/chondrogen_page_title_set.json"));
    assert!(tsx.contains("depth2?: boolean"), "{tsx}");
    assert!(tsx.contains("depth3?: boolean"), "{tsx}");
    assert!(tsx.contains("deapth2?: string"), "{tsx}");
}

#[test]
fn selected_effect_uses_a_valid_selector() {
    let tsx = generate(include_str!(
        "fixtures/chondrogen_list_type_button_set.json"
    ));
    assert!(!tsx.contains("_selected="), "{tsx}");
    assert!(tsx.contains("aria-selected"), "{tsx}");
}

#[test]
fn icon_effects_never_emit_attributes_inside_style_selectors() {
    let tsx = generate(include_str!("fixtures/chondrogen_icon_button_set.json"));
    assert!(!tsx.contains("_hoevr="), "{tsx}");
    assert!(!tsx.contains("\"src\":"), "{tsx}");
}

#[test]
fn desktop_only_footer_logo_has_scalar_source_and_explicit_visibility() {
    let tsx = generate(include_str!("fixtures/chondrogen_footer_set.json"));
    assert!(!tsx.contains("src={["), "{tsx}");
    let image = tsx
        .split("<Image")
        .nth(1)
        .unwrap()
        .split("/>")
        .next()
        .unwrap();
    assert!(image.contains("src=\"/icons/logo.svg\""), "{image}");
    assert!(
        image.contains("display={[\"none\", null, null, null, \"inline\"]}"),
        "{image}"
    );
    assert!(!tsx.contains("typography={["), "{tsx}");
}

#[test]
fn header_container_does_not_inherit_a_leaf_variants_source() {
    let tsx = generate(include_str!("fixtures/chondrogen_header_set.json"));
    let root = tsx
        .split("<Center")
        .nth(1)
        .unwrap()
        .split("<Flex")
        .next()
        .unwrap();
    assert!(!root.contains("src="), "{root}");
    assert!(tsx.contains("<Text"), "{tsx}");
}

#[test]
fn mobile_only_source_is_hidden_at_desktop_instead_of_inherited() {
    let mut snapshot: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/chondrogen_footer_set.json")).unwrap();
    for node in snapshot["nodes"].as_object_mut().unwrap().values_mut() {
        if let Some(default) =
            node.pointer_mut("/fields/componentPropertyDefinitions/viewport/defaultValue")
        {
            *default = serde_json::json!("Mobile");
        }
        if let Some(viewport) = node.pointer_mut("/fields/variantProperties/viewport") {
            *viewport = serde_json::json!(if viewport == "Mobile" {
                "Desktop"
            } else {
                "Mobile"
            });
        }
    }
    let tsx = generate(&snapshot.to_string());
    assert!(!tsx.contains("src={["), "{tsx}");
    assert!(
        tsx.contains("display={[\"inline\", null, null, null, \"none\"]}"),
        "{tsx}"
    );
}

#[test]
fn chondrogen_scalar_props_and_image_elements_are_valid() {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for entry in std::fs::read_dir(fixtures).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if !name.starts_with("chondrogen_") || !name.ends_with("_set.json") {
            continue;
        }
        let tsx = generate(&std::fs::read_to_string(&path).unwrap());
        for scalar in ["src", "typography"] {
            assert!(!tsx.contains(&format!("{scalar}={{[")), "{name}: {tsx}");
        }
        for element in tsx.split('<').skip(1) {
            let tag = element.split_whitespace().next().unwrap_or_default();
            let attributes = element.split('>').next().unwrap_or_default();
            if attributes
                .lines()
                .any(|line| line.trim_start().starts_with("src="))
            {
                assert_eq!(tag, "Image", "{name}: {attributes}");
                assert!(attributes.trim_end().ends_with('/'), "{name}: {attributes}");
            }
        }
    }
}
