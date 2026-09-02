use devup_mcp_devup_ui::codegen::{
    extract_custom_component_imports, extract_devup_imports, generate_import_statements,
    render_codegen_provider, render_component_usage, render_variant_tree_merge,
    render_viewport_component,
};
use serde_json::json;

#[test]
fn import_metadata_is_sorted_deduplicated_and_stable() {
    let components = vec![
        json!({
            "name": "Card",
            "metadata": {
                "devupImports": ["Flex", "Box"],
                "customImports": ["Zebra", "Button"],
                "usesKeyframes": true
            }
        }),
        json!({
            "name": "Panel",
            "metadata": {
                "devupImports": ["Box", "Text"],
                "customImports": ["Button", "Card"],
                "usesKeyframes": false
            }
        }),
    ];

    assert_eq!(
        extract_devup_imports(&components),
        ["Box", "Flex", "Text", "keyframes"]
    );
    assert_eq!(
        extract_custom_component_imports(&components),
        ["Button", "Card", "Zebra"]
    );
    let imports = generate_import_statements(&components);
    assert_eq!(
        imports,
        concat!(
            "import { Box, Flex, Text, keyframes } from '@devup-ui/react'\n",
            "import { Button } from '@/components/Button'\n",
            "import { Card } from '@/components/Card'\n",
            "import { Zebra } from '@/components/Zebra'\n\n",
        )
    );
    assert_eq!(generate_import_statements(&components), imports);
    assert_eq!(generate_import_statements(&[]), "");
}

#[test]
fn component_usage_preserves_variants_boolean_and_text_properties() {
    let component_set = json!({
        "type": "COMPONENT_SET",
        "name": "MyButton",
        "componentPropertyDefinitions": {
            "variant#123:456": {"type": "VARIANT", "defaultValue": "primary"},
            "viewport": {"type": "VARIANT", "defaultValue": "desktop"},
            "hasIcon": {"type": "BOOLEAN", "defaultValue": true},
            "hidden": {"type": "BOOLEAN", "defaultValue": false},
            "label#80:456": {"type": "TEXT", "defaultValue": "Click me"},
            "icon": {"type": "INSTANCE_SWAP", "defaultValue": "some-id"}
        }
    });
    assert_eq!(
        render_component_usage(&component_set).as_deref(),
        Some("<MyButton variant=\"primary\" hasIcon>Click me</MyButton>")
    );

    let instance = json!({
        "type": "INSTANCE",
        "name": "PrimaryButton",
        "componentProperties": {
            "variant#1:2": {"type": "VARIANT", "value": "secondary"},
            "size#3:4": {"type": "VARIANT", "value": "lg"},
            "label#5:6": {"type": "TEXT", "value": "Save"}
        }
    });
    let usage = render_component_usage(&instance).expect("instance usage");
    assert!(usage.starts_with("<PrimaryButton "));
    assert!(usage.contains("variant=\"secondary\""));
    assert!(usage.contains("size=\"lg\""));
    assert!(usage.ends_with(">Save</PrimaryButton>"));

    assert!(render_component_usage(&json!({"type": "FRAME", "name": "Screen"})).is_none());
}

#[test]
fn responsive_provider_covers_sections_variants_and_pure_code_first() {
    let section = json!({
        "language": "devup-ui",
        "node": {
            "type": "SECTION",
            "name": "Responsive Section",
            "children": [{"type": "FRAME"}, {"type": "FRAME"}]
        }
    });
    let provider = render_codegen_provider(&section, "<Box />").expect("provider output");
    assert!(provider.find("Pure Code") < provider.find("ResponsiveSection - Responsive"));

    let viewport = json!({
        "type": "COMPONENT_SET",
        "name": "Icon",
        "componentPropertyDefinitions": {
            "viewport": {"type": "VARIANT", "variantOptions": ["mobile", "desktop"]}
        },
        "children": [
            {"name": "mobile", "isAsset": false, "variantProperties": {"viewport": "mobile"}},
            {"name": "desktop", "isAsset": false, "variantProperties": {"viewport": "desktop"}}
        ]
    });
    let viewport = render_viewport_component(&viewport).expect("viewport output");
    assert!(viewport.contains("'mobile' | 'desktop'"));
    assert!(viewport.contains("src="));

    let merged = render_variant_tree_merge(&json!({
        "variantKey": "size",
        "treesByVariant": [
            ["sm", {"component": "Box", "props": {"p": "4px"}, "children": []}],
            ["lg", {"component": "Box", "props": {"p": "8px"}, "children": []}]
        ]
    }))
    .expect("merged responsive tree");
    assert!(merged.contains("__variantProp"));
    assert!(merged.contains("\"size\""));
}
