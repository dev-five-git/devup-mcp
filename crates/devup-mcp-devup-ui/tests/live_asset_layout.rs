use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::{Snapshot, discover_asset_manifest};
use serde_json::json;

fn captured() -> Snapshot {
    serde_json::from_str(include_str!(
        "fixtures/chondrogen_mobile_header_single_logo.json"
    ))
    .unwrap()
}

#[test]
fn captured_fill_header_row_keeps_fixed_logo_as_its_own_exported_asset() {
    let snapshot = captured();
    let wrapper = "I51:6610;44:5194";
    let logo = "I51:6610;44:5195";
    let manifest = discover_asset_manifest(&snapshot);
    assert!(
        !manifest.assets.iter().any(|a| a.node_id == wrapper),
        "{:?}",
        manifest.assets
    );
    assert!(
        manifest.assets.iter().any(|a| a.node_id == logo),
        "{:?}",
        manifest.assets
    );
    let output = generate_component(
        &snapshot,
        wrapper,
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(output.tsx.contains("<Flex"), "{}", output.tsx);
    assert!(output.tsx.contains("<Image"), "{}", output.tsx);
    assert!(output.tsx.contains("w=\"173px\""), "{}", output.tsx);
    assert!(output.tsx.contains("h=\"28px\""), "{}", output.tsx);
    assert!(
        output.tsx.contains("justifyContent=\"space-between\""),
        "{}",
        output.tsx
    );
    assert!(
        !output.tsx.contains("justifyContent=\"center\""),
        "{}",
        output.tsx
    );
    assert!(
        output
            .source_map
            .entries
            .iter()
            .any(|e| e.node_id.as_deref() == Some(logo) && e.property.is_none())
    );
    if let Ok(path) = std::env::var("DEVUP_LOGO_PROBE_OUTPUT") {
        std::fs::write(path, &output.tsx).unwrap();
    }
}

#[test]
fn shared_manifest_and_renderer_keep_responsive_single_child_sizing() {
    for mode in ["HORIZONTAL", "VERTICAL"] {
        let snapshot:Snapshot=serde_json::from_value(json!({"fileKey":"single-child-live","roots":["row"],"diagnostics":[],"nodes":{
            "row":{"id":"row","type":"FRAME","fields":{"name":"Layout row","childrenIds":["icon"],"layoutMode":mode,"layoutSizingHorizontal":"FILL","width":300,"height":28,"inferredAutoLayout":{"layoutMode":mode}}},
            "icon":{"id":"icon","type":"VECTOR","fields":{"name":"Logo","parentId":"row","width":173,"height":28,"layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED","fills":[{"type":"SOLID","color":{"r":1,"g":0,"b":0}}]}}
        }})).unwrap();
        let manifest = discover_asset_manifest(&snapshot);
        assert_eq!(manifest.assets.len(), 1);
        assert_eq!(manifest.assets[0].node_id, "icon");
        let output = generate_component(&snapshot, "row", &CodegenOptions::default()).unwrap();
        assert!(output.tsx.contains("w=\"173px\""), "{}", output.tsx);
        assert!(
            output
                .source_map
                .entries
                .iter()
                .any(|e| e.node_id.as_deref() == Some("icon") && e.property.is_none())
        );
    }
}

#[test]
fn original_wquw_fill_wrapper_does_not_scale_its_fixed_logo() {
    let snapshot: Snapshot =
        serde_json::from_str(include_str!("../../../fixtures/wquw-119-snapshot.json")).unwrap();
    let manifest = discover_asset_manifest(&snapshot);
    assert!(!manifest.assets.iter().any(|a| a.node_id == "3997:46317"));
    assert!(manifest.assets.iter().any(|a| a.node_id == "3997:46318"));
    let output = generate_component(&snapshot, "3997:46315", &CodegenOptions::default()).unwrap();
    assert!(
        output
            .source_map
            .entries
            .iter()
            .any(|e| e.node_id.as_deref() == Some("3997:46318") && e.property.is_none())
    );
    assert!(output.tsx.contains("w=\"127px\""), "{}", output.tsx);
    assert!(output.tsx.contains("h=\"48px\""), "{}", output.tsx);
    assert!(
        !output.tsx.contains("aspectRatio=\"320 / 48\""),
        "{}",
        output.tsx
    );
}
