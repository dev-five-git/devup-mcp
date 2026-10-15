use devup_mcp_devup_ui::codegen::{AssetRepresentationPolicy, CodegenOptions, generate_component};
use devup_mcp_figma::{AssetRenderMode, Snapshot};
use serde_json::{Value, json};

fn picture(mode: &str, transform: Value, width: u32, height: u32) -> Snapshot {
    serde_json::from_value(json!({
        "fileKey":"crop-export", "version":"1", "roots":["picture"], "diagnostics":[],
        "nodes":{"picture":{"id":"picture","type":"FRAME","fields":{
            "name":"Portrait", "isAsset":true,
            "width":width, "height":height,
            "layoutSizingHorizontal":"FIXED", "layoutSizingVertical":"FIXED",
            "fills":[
                {"type":"SOLID","color":{"r":0.9,"g":0.9,"b":0.9}},
                {"type":"IMAGE","imageHash":"photo","scaleMode":mode,
                 "imageTransform":transform}
            ]
        }}}
    }))
    .unwrap()
}

// Explicit native renditions have already applied the source crop. The
// default source rendition instead needs that crop projected in CSS.
#[test]
fn cropped_node_export_does_not_apply_the_source_transform_twice() {
    for (transform, width, height) in [
        (json!([[0.5, 0, 0.25], [0, 0.8, 0.1]]), 240, 180),
        (json!([[1.1, 0, -0.05], [0, 0.7, 0.2]]), 513, 271),
        (json!([[0, -1, 1], [1, 0, 0]]), 137, 219),
        (Value::Null, 211, 149),
    ] {
        let snapshot = picture("CROP", transform, width, height);
        let output = generate_component(
            &snapshot,
            "picture",
            &CodegenOptions {
                asset_policies: std::collections::BTreeMap::from([(
                    "picture:fills:1".into(),
                    AssetRepresentationPolicy {
                        render_mode: AssetRenderMode::Node,
                        effects_baked: Some(true),
                        asset_path: Some("/images/Portrait-1.png".into()),
                        ..Default::default()
                    },
                )]),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            output.tsx.contains("src=\"/images/Portrait-1.png\""),
            "{}",
            output.tsx
        );
        assert!(output.tsx.contains("objectFit=\"fill\""));
        assert!(!output.tsx.contains("transform="));
    }
}

#[test]
fn non_crop_background_modes_keep_their_existing_behavior() {
    for (mode, expected) in [
        ("FILL", "objectFit=\"cover\""),
        ("FIT", "objectFit=\"contain\""),
        ("TILE", "repeat"),
    ] {
        let snapshot = picture(mode, json!([[0.5, 0, 0.25], [0, 0.8, 0.1]]), 240, 180);
        let output = generate_component(&snapshot, "picture", &CodegenOptions::default()).unwrap();
        assert!(output.tsx.contains(expected), "{}", output.tsx);
    }
}

#[test]
fn cropped_export_background_maps_to_its_actual_fill_and_asset() {
    let snapshot = picture("CROP", json!([[0.5, 0, 0.25], [0, 0.8, 0.1]]), 240, 180);
    let output = generate_component(&snapshot, "picture", &CodegenOptions::default()).unwrap();
    assert!(output.source_map.entries.iter().any(|entry| {
        entry.node_id.as_deref() == Some("picture")
            && entry.property.as_deref() == Some("fills")
            && entry
                .generated_property
                .as_deref()
                .is_some_and(|property| property.contains("translate(-50%, -12.5%)"))
    }));
    assert!(
        output
            .source_map
            .entries
            .iter()
            .any(|entry| { entry.asset_id.as_deref() == Some("picture:fills:1") })
    );
    assert!(output.fidelity_report.assets.complete());
    let mut wrong = output.clone();
    wrong.tsx = wrong
        .tsx
        .replace("/images/Portrait-1.png", "/images/unrelated.png");
    let report =
        devup_mcp_devup_ui::provenance::validate_fidelity(&snapshot, "picture", &wrong).unwrap();
    assert!(!report.assets.complete());
}
