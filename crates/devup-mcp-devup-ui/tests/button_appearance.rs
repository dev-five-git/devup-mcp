use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::json;

#[test]
fn white_outline_button_without_fill_disables_the_native_face() {
    for (name, tag) in [("Button / Outline", None), ("CTA", Some("button"))] {
        for fill in [
            json!([]),
            json!([{"type":"SOLID","color":{"r":0,"g":0,"b":0}}]),
        ] {
            let snapshot: Snapshot = serde_json::from_value(json!({
                "fileKey":"button", "roots":["button"], "diagnostics":[], "nodes": {
                    "button":{"id":"button","type":"FRAME","fields":{
                        "name":name,"as":tag, "width":160,"height":48,"layoutMode":"HORIZONTAL",
                        "fills":fill,"strokeWeight":1,"strokeAlign":"INSIDE",
                        "strokes":[{"type":"SOLID","color":{"r":1,"g":1,"b":1}}],
                        "childrenIds":["label"]
                    }},
                    "label":{"id":"label","type":"TEXT","fields":{
                        "characters":"Get started","fontSize":16,
                        "fills":[{"type":"SOLID","color":{"r":1,"g":1,"b":1}}]
                    }}
                }
            }))
            .unwrap();
            let output =
                generate_component(&snapshot, "button", &CodegenOptions::default()).unwrap();
            assert!(output.tsx.contains("appearance=\"none\""), "{}", output.tsx);
            assert!(output.tsx.contains("color=\"#FFF\""), "{}", output.tsx);
            assert!(
                output.tsx.contains("border=\"solid 1px #FFF\""),
                "{}",
                output.tsx
            );
            let expected = if fill.as_array().unwrap().is_empty() {
                "transparent"
            } else {
                "#000"
            };
            assert!(
                output.tsx.contains(&format!("bg=\"{expected}\"")),
                "{}",
                output.tsx
            );
            assert_eq!(output.tsx.matches("bg=").count(), 1, "{}", output.tsx);
            if tag.is_some() {
                assert!(output.tsx.contains("as=\"button\""), "{}", output.tsx);
            }
        }
    }
}

#[test]
fn button_variant_inherits_native_appearance_safety_from_its_component_set() {
    let snapshot: Snapshot = serde_json::from_value(json!({
        "fileKey":"button","roots":["variant"],"diagnostics":[],"nodes":{
            "set":{"id":"set","type":"COMPONENT_SET","fields":{
                "name":"Button","childrenIds":["variant"]
            }},
            "variant":{"id":"variant","type":"COMPONENT","fields":{
                "name":"Style=Outline","width":160,"height":48,"fills":[]
            }}
        }
    }))
    .unwrap();
    let output = generate_component(&snapshot, "variant", &CodegenOptions::default()).unwrap();
    assert!(output.tsx.contains("appearance=\"none\""), "{}", output.tsx);
    assert!(output.tsx.contains("bg=\"transparent\""), "{}", output.tsx);
}
