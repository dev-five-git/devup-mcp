use std::collections::BTreeMap;

use devup_mcp_devup_ui::codegen::{
    AssetRepresentationPolicy, CodegenOptions, generate_component, responsive::merge_breakpoints,
};
use devup_mcp_figma::Snapshot;
use serde_json::json;

#[test]
fn actual_desktop_only_header_and_footer_logos_never_use_css_arrays_for_alt() {
    for source in [
        include_str!("fixtures/chondrogen_header_set.json"),
        include_str!("fixtures/chondrogen_footer_set.json"),
    ] {
        let snapshot: Snapshot = serde_json::from_str(source).unwrap();
        let output =
            generate_component(&snapshot, &snapshot.roots[0], &CodegenOptions::default()).unwrap();
        assert!(!output.tsx.contains("alt={["), "{}", output.tsx);
        assert!(output.tsx.contains("alt=\"Logo\""), "{}", output.tsx);
    }
}

fn responsive_snapshot(desktop_only: bool) -> (Snapshot, CodegenOptions) {
    let mut nodes = serde_json::Map::new();
    let mut policies = BTreeMap::new();
    for (suffix, width, name) in [
        ("mobile", 360, "Mobile logo"),
        ("desktop", 1440, "Desktop logo"),
    ] {
        let root = format!("root-{suffix}");
        let image = format!("image-{suffix}");
        let children = if desktop_only && suffix == "mobile" {
            vec![]
        } else {
            vec![image.clone()]
        };
        for node in [
            json!({"id":root,"type":"FRAME","fields":{"name":"Screen","width":width,"height":300,"childrenIds":children,"layoutMode":"VERTICAL"}}),
            json!({"id":image,"type":"RECTANGLE","fields":{"name":name,"parentId":root,"width":200,"height":100,"isAsset":true,"fills":[{"type":"IMAGE","imageHash":"logo","scaleMode":"FILL"}]}}),
        ] {
            nodes.insert(node["id"].as_str().unwrap().into(), node);
        }
        policies.insert(
            format!("{image}:fills:0"),
            AssetRepresentationPolicy {
                asset_path: Some("/same-logo.png".into()),
                ..Default::default()
            },
        );
    }
    let snapshot=serde_json::from_value(json!({"fileKey":"scalar-alt","roots":["root-mobile","root-desktop"],"nodes":nodes,"diagnostics":[]})).unwrap();
    (
        snapshot,
        CodegenOptions {
            asset_policies: policies,
            ..Default::default()
        },
    )
}

#[test]
fn an_image_absent_at_mobile_keeps_its_captured_scalar_alternative() {
    let (snapshot, options) = responsive_snapshot(true);
    let merged = merge_breakpoints(&snapshot, &options).unwrap().unwrap();
    assert!(!merged.tsx.contains("alt={["), "{}", merged.tsx);
    assert!(
        merged.tsx.contains("alt=\"Desktop logo\""),
        "{}",
        merged.tsx
    );
    assert!(
        !merged
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_RESPONSIVE_ALT_UNREPRESENTED")
    );
}

#[test]
fn different_visible_alternatives_are_scalar_but_explicitly_lossy() {
    let (snapshot, options) = responsive_snapshot(false);
    let merged = merge_breakpoints(&snapshot, &options).unwrap().unwrap();
    assert!(!merged.tsx.contains("alt={["), "{}", merged.tsx);
    assert!(
        merged
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_RESPONSIVE_ALT_UNREPRESENTED"),
        "{:?}",
        merged
    );
    assert!(!merged.unrepresented.is_empty());
}

#[test]
fn visible_viewport_variant_alt_conflicts_have_a_loss_diagnostic() {
    let mut nodes = serde_json::Map::new();
    nodes.insert("set".into(),json!({"id":"set","type":"COMPONENT_SET","fields":{"name":"Logo","childrenIds":["desktop","mobile"],"componentPropertyDefinitions":{"viewport":{"type":"VARIANT","defaultValue":"desktop","variantOptions":["desktop","mobile"]}}}}));
    for (viewport, name, width) in [
        ("desktop", "Desktop logo", 200),
        ("mobile", "Mobile logo", 100),
    ] {
        nodes.insert(viewport.into(),json!({"id":viewport,"type":"COMPONENT","fields":{"name":name,"parentId":"set","variantProperties":{"viewport":viewport},"width":width,"height":50,"isAsset":true,"fills":[{"type":"IMAGE","imageHash":"same","scaleMode":"FILL"}]}}));
    }
    let snapshot: Snapshot = serde_json::from_value(
        json!({"fileKey":"scalar-alt-variant","roots":["set"],"nodes":nodes,"diagnostics":[]}),
    )
    .unwrap();
    let output = generate_component(&snapshot, "set", &CodegenOptions::default()).unwrap();
    assert!(!output.tsx.contains("alt={["), "{}", output.tsx);
    let issue = output
        .diagnostics
        .iter()
        .find(|d| d.code == "DEVUP_CODEGEN_RESPONSIVE_ALT_UNREPRESENTED")
        .expect("conflicting captured alternatives need a loss diagnostic");
    assert_eq!(
        issue.fidelity_impact,
        Some(devup_mcp_figma::FidelityImpact::Lossy)
    );
}
