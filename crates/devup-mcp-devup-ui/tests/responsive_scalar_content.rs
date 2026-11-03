use devup_mcp_devup_ui::codegen::{CodegenOptions, responsive::merge_breakpoints};
use devup_mcp_figma::Snapshot;
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn different_typography_tokens_preserve_separate_captured_text_nodes() {
    let mut nodes = serde_json::Map::new();
    for (suffix, width, size, style) in [
        ("mobile", 390, 32, "style3"),
        ("desktop", 1440, 48, "style2"),
    ] {
        let root = format!("root-{suffix}");
        let text = format!("text-{suffix}");
        for node in [
            json!({"id":root,"type":"FRAME","fields":{"name":"Screen","width":width,"height":300,"layoutMode":"VERTICAL","childrenIds":[text]}}),
            json!({"id":text,"type":"TEXT","fields":{"name":"Heading","parentId":root,"characters":"Research","fontSize":size,"textStyleId":style}}),
        ] {
            nodes.insert(node["id"].as_str().unwrap().into(), node);
        }
    }
    let snapshot:Snapshot=serde_json::from_value(json!({"fileKey":"responsive-typography","roots":["root-mobile","root-desktop"],"nodes":nodes,"diagnostics":[]})).unwrap();
    let options = CodegenOptions {
        text_style_tokens: BTreeMap::from([
            ("style3".into(), "h3".into()),
            ("style2".into(), "h2".into()),
        ]),
        ..Default::default()
    };
    let merged = merge_breakpoints(&snapshot, &options).unwrap().unwrap();
    assert!(!merged.tsx.contains("typography={["), "{}", merged.tsx);
    for expected in [
        "typography=\"h3\"",
        "typography=\"h2\"",
        "fontSize=\"32px\"",
        "fontSize=\"48px\"",
    ] {
        assert!(
            merged.tsx.contains(expected),
            "missing {expected}: {}",
            merged.tsx
        );
    }
    assert_eq!(merged.tsx.matches("<Text").count(), 2);
    let mobile = merged
        .elements
        .iter()
        .find(|element| element.ids.iter().any(|(_, id)| id == "text-mobile"))
        .unwrap();
    let desktop = merged
        .elements
        .iter()
        .find(|element| element.ids.iter().any(|(_, id)| id == "text-desktop"))
        .unwrap();
    assert!(mobile.hidden_at.contains(&1440));
    assert!(desktop.hidden_at.contains(&390));
}

#[test]
fn inline_responsive_instances_resolve_ancestor_boolean_visibility() {
    let mut nodes = serde_json::Map::new();
    for (suffix, width) in [("mobile", 390), ("desktop", 1440)] {
        let root = format!("root-{suffix}");
        let instance = format!("field-{suffix}");
        let input = format!("input-{suffix}");
        let textarea = format!("textarea-{suffix}");
        let error = format!("error-{suffix}");
        let text = format!("text-{suffix}");
        for node in [
            json!({"id":root,"type":"FRAME","fields":{"name":"Screen","width":width,"height":300,"layoutMode":"VERTICAL","childrenIds":[instance]}}),
            json!({"id":instance,"type":"INSTANCE","fields":{"name":"FormField","parentId":root,"layoutMode":"VERTICAL","childrenIds":[input,textarea,error],"componentProperties":{"showInput#1":{"type":"BOOLEAN","value":false},"showTextarea#2":{"type":"BOOLEAN","value":true},"showError#3":{"type":"BOOLEAN","value":false}}}}),
            json!({"id":input,"type":"TEXT","fields":{"name":"Input","parentId":instance,"characters":"Hidden input placeholder","visible":true,"fontSize":16,"componentPropertyReferences":{"visible":"showInput#1"}}}),
            json!({"id":textarea,"type":"FRAME","fields":{"name":"Textarea","parentId":instance,"visible":false,"layoutMode":"VERTICAL","childrenIds":[text],"componentPropertyReferences":{"visible":"showTextarea#2"}}}),
            json!({"id":text,"type":"TEXT","fields":{"name":"Content","parentId":textarea,"characters":"Visible textarea content","fontSize":16}}),
            json!({"id":error,"type":"TEXT","fields":{"name":"Error","parentId":instance,"characters":"Hidden error text","visible":true,"fontSize":16,"componentPropertyReferences":{"visible":"showError#3"}}}),
        ] {
            nodes.insert(node["id"].as_str().unwrap().into(), node);
        }
    }
    let snapshot:Snapshot=serde_json::from_value(json!({"fileKey":"responsive-boolean","roots":["root-mobile","root-desktop"],"nodes":nodes,"diagnostics":[]})).unwrap();
    for root in ["root-mobile", "root-desktop"] {
        let output = devup_mcp_devup_ui::codegen::generate_component(
            &snapshot,
            root,
            &CodegenOptions {
                inline_instances: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            !output.tsx.contains("Hidden input placeholder"),
            "{}",
            output.tsx
        );
        assert!(!output.tsx.contains("Hidden error text"), "{}", output.tsx);
        assert!(
            output.tsx.contains("Visible textarea content"),
            "{}",
            output.tsx
        );
    }
    let merged = merge_breakpoints(
        &snapshot,
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    assert!(
        !merged.tsx.contains("Hidden input placeholder"),
        "{}",
        merged.tsx
    );
    assert!(!merged.tsx.contains("Hidden error text"), "{}", merged.tsx);
    assert!(
        merged.tsx.contains("Visible textarea content"),
        "{}",
        merged.tsx
    );
}
