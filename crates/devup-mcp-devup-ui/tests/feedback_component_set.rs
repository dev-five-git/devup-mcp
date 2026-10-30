use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::json;

#[test]
fn component_set_exports_one_component_with_variant_and_text_props() {
    let mut nodes = json!({
        "set": {"id":"set", "type":"COMPONENT_SET", "fields": {
            "name":"Button", "width":900, "height":600,
            "childrenIds":["small", "large"],
            "componentPropertyDefinitions": {
                "size":{"type":"VARIANT", "defaultValue":"sm", "variantOptions":["sm","md"]},
                "label#1":{"type":"TEXT", "defaultValue":"Submit"}
            }
        }, "extra":{}, "fieldErrors":{}}
    });
    for (id, size, width) in [("small", "sm", 80), ("large", "md", 120)] {
        let text = format!("{id}-text");
        nodes[id] = json!({"id":id,"type":"COMPONENT","fields":{
            "name":format!("size={size}"), "parentId":"set", "width":width,"height":40,
            "layoutSizingHorizontal":"FIXED", "layoutSizingVertical":"FIXED",
            "variantProperties":{"size":size},"childrenIds":[text]
        },"extra":{},"fieldErrors":{}});
        nodes[&text] = json!({"id":text,"type":"TEXT","fields":{
            "parentId":id,"characters":"Submit","width":60,"height":20,
            "componentPropertyReferences":{"characters":"label#1"}
        },"extra":{},"fieldErrors":{}});
    }
    let snapshot: Snapshot = serde_json::from_value(json!({
        "fileKey":"FileKey123","version":"v1","roots":["set"],"nodes":nodes,"diagnostics":[]
    }))
    .unwrap();
    let output = generate_component(&snapshot, "set", &CodegenOptions::default()).unwrap();
    assert_eq!(
        output.tsx.matches("export function Button").count(),
        1,
        "{}",
        output.tsx
    );
    assert!(output.tsx.contains("size:"), "{}", output.tsx);
    assert!(output.tsx.contains("label?: string"), "{}", output.tsx);
    assert!(output.tsx.contains("{label}"), "{}", output.tsx);
    assert!(!output.tsx.contains("900px"), "{}", output.tsx);
    assert!(output.tsx.contains("[size]"), "{}", output.tsx);
}

#[test]
fn interaction_state_variants_become_pseudo_styles() {
    let snapshot: Snapshot = serde_json::from_value(json!({
        "fileKey":"FileKey123","version":"v1","roots":["set"],"diagnostics":[],
        "nodes":{
            "set":{"id":"set","type":"COMPONENT_SET","fields":{
                "name":"Button","childrenIds":["default","hover"],
                "componentPropertyDefinitions":{"State":{"type":"VARIANT","defaultValue":"Default","variantOptions":["Default","Hover"]}}
            },"extra":{},"fieldErrors":{}},
            "default":{"id":"default","type":"COMPONENT","fields":{
                "name":"State=Default","variantProperties":{"State":"Default"},
                "fills":[{"type":"SOLID","color":{"r":1,"g":0,"b":0}}]
            },"extra":{},"fieldErrors":{}},
            "hover":{"id":"hover","type":"COMPONENT","fields":{
                "name":"State=Hover","variantProperties":{"State":"Hover"},
                "fills":[{"type":"SOLID","color":{"r":0,"g":0,"b":1}}]
            },"extra":{},"fieldErrors":{}}
        }
    })).unwrap();
    let output = generate_component(&snapshot, "set", &CodegenOptions::default()).unwrap();
    assert!(output.tsx.contains("_hover="), "{}", output.tsx);
    assert!(!output.tsx.contains("State:"), "{}", output.tsx);
}

#[test]
fn component_set_preserves_fixed_leaf_bar_height() {
    let snapshot: Snapshot = serde_json::from_value(json!({
        "fileKey":"FileKey123","version":"v1","roots":["set"],"diagnostics":[],
        "nodes":{
            "set":{"id":"set","type":"COMPONENT_SET","fields":{
                "name":"TechStep","childrenIds":["step"],
                "componentPropertyDefinitions":{"step":{"type":"VARIANT","defaultValue":"one","variantOptions":["one"]}}
            },"extra":{},"fieldErrors":{}},
            "step":{"id":"step","type":"COMPONENT","fields":{
                "name":"step=one","variantProperties":{"step":"one"},
                "layoutMode":"VERTICAL","width":200,"height":100,"childrenIds":["label","bar"]
            },"extra":{},"fieldErrors":{}},
            "label":{"id":"label","type":"TEXT","fields":{
                "parentId":"step","characters":"Step one","width":80,"height":20
            },"extra":{},"fieldErrors":{}},
            "bar":{"id":"bar","type":"RECTANGLE","fields":{
                "parentId":"step","width":160,"height":4,
                "layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED",
                "fills":[{"type":"SOLID","color":{"r":1,"g":0,"b":0}}]
            },"extra":{},"fieldErrors":{}}
        }
    })).unwrap();
    let output = generate_component(&snapshot, "set", &CodegenOptions::default()).unwrap();
    assert!(output.tsx.contains("h=\"4px\""), "{}", output.tsx);
    assert!(output.tsx.contains("w=\"160px\""), "{}", output.tsx);
}
