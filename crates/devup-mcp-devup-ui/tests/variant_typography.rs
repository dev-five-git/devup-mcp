//! `typography` is the one prop a variant map has to be frozen for.
//!
//! Built here rather than captured. A design token only reaches the converter
//! through the style table that comes with a real acquisition, and the captures
//! that carry one are not committed — so the set is written out, which also
//! means this rule is checked wherever the tests run rather than only where
//! someone has a capture.

use std::collections::BTreeMap;

use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component_set_target};
use devup_mcp_figma::Snapshot;
use serde_json::json;

/// A set with one real dimension, `size`, and the `effect` dimension that makes
/// it a set worth folding. Each size labels its text with a different style.
fn button_set() -> Snapshot {
    let mut nodes = json!({
        "set": {
            "id": "set",
            "type": "COMPONENT_SET",
            "fields": {
                "name": "Label",
                "childrenIds": ["lg-default", "sm-default"],
                "componentPropertyDefinitions": {
                    "size": {
                        "type": "VARIANT",
                        "defaultValue": "lg",
                        "variantOptions": ["lg", "sm"]
                    },
                    "effect": {
                        "type": "VARIANT",
                        "defaultValue": "default",
                        "variantOptions": ["default"]
                    }
                }
            },
            "extra": {},
            "fieldErrors": {}
        }
    });
    for (size, style) in [("lg", "style-lg"), ("sm", "style-sm")] {
        let variant = format!("{size}-default");
        let text = format!("{size}-text");
        nodes[&variant] = json!({
            "id": variant,
            "type": "COMPONENT",
            "fields": {
                "name": format!("size={size}, effect=default"),
                "parentId": "set",
                "childrenIds": [text],
                "variantProperties": {"size": size, "effect": "default"},
                "width": 100, "height": 20
            },
            "extra": {},
            "fieldErrors": {}
        });
        nodes[&text] = json!({
            "id": text,
            "type": "TEXT",
            "fields": {
                "name": "label",
                "parentId": variant,
                "childrenIds": [],
                "characters": "Label",
                "width": 80, "height": 16,
                "styledTextSegments": [{"characters": "Label", "textStyleId": style}]
            },
            "extra": {},
            "fieldErrors": {}
        });
    }
    serde_json::from_value(json!({
        "fileKey": "FileKey123",
        "version": "v1",
        "roots": ["set"],
        "nodes": nodes,
        "diagnostics": []
    }))
    .expect("snapshot")
}

fn generated() -> String {
    let options = CodegenOptions {
        component_name: Some("Label".to_owned()),
        text_style_tokens: BTreeMap::from([
            ("style-lg".to_owned(), "buttonLg".to_owned()),
            ("style-sm".to_owned(), "buttonSm".to_owned()),
        ]),
        ..CodegenOptions::default()
    };
    generate_component_set_target(&button_set(), "set", "Label", &options)
        .expect("codegen")
        .tsx
}

/// devup-ui types `typography` by the literal it is given. A map read at
/// runtime widens every entry to `string`, and the token stops being one, so
/// the object is frozen before it is indexed.
#[test]
fn a_typography_map_is_frozen_before_it_is_indexed() {
    let tsx = generated();
    assert!(
        tsx.contains(
            r#"typography={({
          lg: "buttonLg",
          sm: "buttonSm"
        } as const)[size]}"#
        ),
        "{tsx}"
    );
}

/// And no other prop is, because no other prop needs it. Widening a colour or
/// a length to `string` costs nothing.
#[test]
fn nothing_else_is_frozen() {
    let tsx = generated();
    assert_eq!(tsx.matches("as const").count(), 1, "{tsx}");
}
