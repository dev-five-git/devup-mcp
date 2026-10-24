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
