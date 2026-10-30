use devup_mcp_devup_ui::codegen::{CodegenOptions, LengthToken, generate_component};
use devup_mcp_figma::{Snapshot, discover_asset_manifest};
use serde_json::{Value, json};

fn snapshot(nodes: Value) -> Snapshot {
    serde_json::from_value(
        json!({"fileKey":"regression","roots":["root"],"nodes":nodes,"diagnostics":[]}),
    )
    .unwrap()
}

#[test]
fn auto_layout_assets_preserve_container_and_individual_icons() {
    for kind in ["FRAME", "INSTANCE", "COMPONENT"] {
        for mode in ["HORIZONTAL", "VERTICAL", "GRID"] {
            let input = snapshot(json!({
                "root":{"id":"root","type":kind,"fields":{"name":"Header","layoutMode":mode,"width":360,"height":64,"paddingLeft":12,"itemSpacing":29,"childrenIds":["logo","menu"]}},
                "logo":{"id":"logo","type":"VECTOR","fields":{"name":"Logo","parentId":"root","width":120,"height":24}},
                "menu":{"id":"menu","type":"VECTOR","fields":{"name":"Menu","parentId":"root","width":24,"height":24}}
            }));
            let output = generate_component(&input, "root", &CodegenOptions::default()).unwrap();
            assert!(
                !output.tsx.contains("/icons/header.svg"),
                "{kind}/{mode}: {}",
                output.tsx
            );
            let manifest = discover_asset_manifest(&input);
            assert_eq!(manifest.assets.len(), 2, "{kind}/{mode}: {manifest:?}");
        }
    }
}

#[test]
fn exported_root_never_inherits_absolute_placement_from_uncollected_parent() {
    let input = snapshot(
        json!({"root":{"id":"root","type":"FRAME","fields":{"layoutPositioning":"ABSOLUTE","parentId":"outside","width":360,"height":64,"layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED"}}}),
    );
    let output = generate_component(&input, "root", &CodegenOptions::default()).unwrap();
    assert!(!output.tsx.contains("pos=\"absolute\""), "{}", output.tsx);
}

#[test]
fn root_fill_and_hug_axes_are_independent() {
    for (horizontal, vertical, width, height) in [
        ("FILL", "HUG", true, false),
        ("HUG", "FILL", false, true),
        ("HUG", "HUG", false, false),
        ("FILL", "FILL", true, true),
    ] {
        let input = snapshot(
            json!({"root":{"id":"root","type":"FRAME","fields":{"layoutMode":"VERTICAL","layoutSizingHorizontal":horizontal,"layoutSizingVertical":vertical,"width":360,"height":80}}}),
        );
        let output = generate_component(&input, "root", &CodegenOptions::default()).unwrap();
        assert_eq!(
            output.tsx.contains("w=\"100%\"") || output.tsx.contains("boxSize=\"100%\""),
            width,
            "{}",
            output.tsx
        );
        assert_eq!(
            output.tsx.contains("h=\"100%\"") || output.tsx.contains("boxSize=\"100%\""),
            height,
            "{}",
            output.tsx
        );
    }
}

#[test]
fn typography_deduplicates_resolved_tokens_and_retains_real_overrides() {
    for size in [16, 18] {
        let input = snapshot(
            json!({"root":{"id":"root","type":"TEXT","fields":{"width":360,"characters":"Body","textStyleId":"style","fontSize":size,"fontWeight":400,"lineHeight":{"unit":"PIXELS","value":24},"letterSpacing":{"unit":"PIXELS","value":1},"boundVariables":{"fontSize":{"id":"size","type":"VARIABLE_ALIAS"}}}}}),
        );
        let options = CodegenOptions {
            text_style_tokens: [("style".into(), "body".into())].into(),
            typography_values: [("body".into(), json!({"fontSize":"$body","fontWeight":400,"lineHeight":"24px","letterSpacing":"0px"}))].into(),
            length_tokens: [("size".into(), LengthToken {token:"body".into(),value:json!("16px")})].into(),
            ..Default::default()
        };
        let output = generate_component(&input, "root", &options).unwrap();
        assert_eq!(
            output.tsx.contains("fontSize="),
            size != 16,
            "{}",
            output.tsx
        );
        assert!(!output.tsx.contains("fontWeight="), "{}", output.tsx);
        assert!(!output.tsx.contains("lineHeight="), "{}", output.tsx);
        assert!(
            output.tsx.contains("letterSpacing=\"1px\""),
            "{}",
            output.tsx
        );
    }
}

#[test]
fn suspicious_values_are_advisory_and_preserve_original_dimensions() {
    let input = snapshot(
        json!({"root":{"id":"root","type":"FRAME","fields":{"width":171,"height":80,"layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED","targetAspectRatio":{"x":6.19,"y":1}}}}),
    );
    let output = generate_component(&input, "root", &CodegenOptions::default()).unwrap();
    let issues = output
        .diagnostics
        .iter()
        .filter(|d| d.code == "DEVUP_CODEGEN_SUSPICIOUS_VALUE")
        .collect::<Vec<_>>();
    assert_eq!(issues.len(), 2, "{:?}", output.diagnostics);
    assert!(
        issues
            .iter()
            .all(|issue| issue.fidelity_impact() == devup_mcp_figma::FidelityImpact::None)
    );
    assert!(
        issues
            .iter()
            .all(|issue| issue.details.as_ref().unwrap()["nextAction"].is_string())
    );
    assert!(output.tsx.contains("w=\"171px\""), "{}", output.tsx);
    assert!(
        output.tsx.contains("aspectRatio=\"6.19\""),
        "{}",
        output.tsx
    );
}

#[test]
fn suspicious_values_exclude_centered_arithmetic_text_and_simple_ratios() {
    for mode in ["centered", "arithmetic", "text", "asset", "simple"] {
        let mut input = snapshot(json!({
            "root":{"id":"root","type":"FRAME","fields":{"width":360,"height":80,"layoutMode":"HORIZONTAL","paddingLeft":9,"paddingRight":9,"itemSpacing":0,"childrenIds":["child","sibling"]}},
            "child":{"id":"child","type":"FRAME","fields":{"parentId":"root","width":171,"height":80,"layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED"}},
            "sibling":{"id":"sibling","type":"TEXT","fields":{"parentId":"root","characters":"Sibling","width":171}}
        }));
        match mode {
            "centered" => {
                input
                    .nodes
                    .get_mut("root")
                    .unwrap()
                    .fields
                    .insert("counterAxisAlignItems".into(), json!("CENTER"));
            }
            "arithmetic" => {}
            "text" => {
                let node = input.nodes.get_mut("child").unwrap();
                node.node_type = "TEXT".into();
                node.fields
                    .insert("layoutSizingHorizontal".into(), json!("HUG"));
            }
            "asset" => {
                input.nodes.get_mut("child").unwrap().node_type = "VECTOR".into();
            }
            "simple" => {
                let node = input.nodes.get_mut("child").unwrap();
                node.fields.insert("width".into(), json!(160));
                node.fields
                    .insert("targetAspectRatio".into(), json!({"x":16,"y":9}));
            }
            _ => unreachable!(),
        }
        let output = generate_component(&input, "root", &CodegenOptions::default()).unwrap();
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|d| d.code == "DEVUP_CODEGEN_SUSPICIOUS_VALUE"
                    && d.node_id.as_deref() == Some("child")),
            "{mode}: {:?}",
            output.diagnostics
        );
    }
}
