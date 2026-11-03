use devup_mcp_devup_ui::codegen::{
    CodegenOptions, generate_component, responsive::merge_confirmed_breakpoints,
};
use devup_mcp_figma::Snapshot;
use serde_json::json;

fn snapshot() -> Snapshot {
    serde_json::from_str(include_str!("fixtures/transferred-image-minimum.json")).unwrap()
}

#[test]
fn captured_ratio_keeps_horizontal_transferred_minimum_and_verified_allocation() {
    let snapshot = snapshot();
    let output = generate_component(
        &snapshot,
        "desktop",
        &CodegenOptions {
            width_policy: Some(Default::default()),
            ..Default::default()
        },
    )
    .unwrap();
    let map = output
        .source_map
        .asset_css_mappings
        .iter()
        .find(|m| m["nodeId"] == "image-desktop")
        .unwrap();
    assert_eq!(map["css"]["wrapper"]["minW"], "auto");
    assert_eq!(
        map["minimumSizingEvidence"]["capturedTransferredMinimum"],
        541.
    );
    assert!(output.tsx.contains("minW=\"auto\""), "{}", output.tsx);
    assert!(
        output
            .source_map
            .entries
            .iter()
            .any(|entry| entry.node_id.as_deref() == Some("image-desktop")
                && entry.property.as_deref() == Some("layoutSizingHorizontal")
                && entry.resolution == "verified-layout-sizing"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn complete_responsive_module_resets_mobile_zero_to_desktop_auto_without_fixed_width() {
    let snapshot = snapshot();
    let output = merge_confirmed_breakpoints(
        &snapshot,
        &CodegenOptions {
            width_policy: Some(Default::default()),
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    assert!(output.tsx.contains("minW={["), "{}", output.tsx);
    assert!(output.tsx.contains("\"auto\""), "{}", output.tsx);
    assert!(!output.tsx.contains("w=\"541px\""), "{}", output.tsx);
    assert!(output.verified, "{:?}", output.gaps);
}

#[test]
fn unknown_ratio_vertical_parent_or_unsized_cross_axis_keep_conservative_zero_minimum() {
    for variation in ["ratio", "parent", "height", "minWidth", "mismatchedRatio"] {
        let mut snapshot = snapshot();
        match variation {
            "ratio" => {
                snapshot
                    .nodes
                    .get_mut("image-desktop")
                    .unwrap()
                    .fields
                    .remove("targetAspectRatio");
            }
            "parent" => {
                snapshot
                    .nodes
                    .get_mut("body-desktop")
                    .unwrap()
                    .fields
                    .insert("layoutMode".into(), json!("VERTICAL"));
            }
            "height" => {
                snapshot
                    .nodes
                    .get_mut("image-desktop")
                    .unwrap()
                    .fields
                    .insert("layoutSizingVertical".into(), json!("HUG"));
            }
            "minWidth" => {
                snapshot
                    .nodes
                    .get_mut("image-desktop")
                    .unwrap()
                    .fields
                    .insert("minWidth".into(), json!(0));
            }
            _ => {
                snapshot
                    .nodes
                    .get_mut("image-desktop")
                    .unwrap()
                    .fields
                    .insert("targetAspectRatio".into(), json!({"x":2,"y":1}));
            }
        }
        let output = generate_component(
            &snapshot,
            "desktop",
            &CodegenOptions {
                width_policy: Some(Default::default()),
                ..Default::default()
            },
        )
        .unwrap();
        let map = output
            .source_map
            .asset_css_mappings
            .iter()
            .find(|m| m["nodeId"] == "image-desktop")
            .unwrap();
        assert_eq!(map["css"]["wrapper"]["minW"], "0", "{variation}");
        assert!(map["minimumSizingEvidence"].is_null(), "{variation}");
    }
    let mobile = generate_component(
        &snapshot(),
        "mobile",
        &CodegenOptions {
            width_policy: Some(Default::default()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        mobile.source_map.asset_css_mappings[0]["css"]["wrapper"]["minW"],
        "0"
    );
}
