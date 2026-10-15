use devup_mcp_devup_ui::codegen::{AssetRepresentationPolicy, CodegenOptions, generate_component};
use devup_mcp_figma::{AssetRenderMode, SnapshotChunk, merge_chunks};
use serde_json::json;

#[test]
fn flattened_subtree_suppresses_own_blur_shadow_and_preserves_layout_ancestor_effects() {
    let shadow = json!({"type":"DROP_SHADOW","visible":true,"radius":5,"spread":0,"offset":{"x":0,"y":2},"color":{"r":0,"g":0,"b":0,"a":0.5}});
    let chunk: SnapshotChunk = serde_json::from_value(json!({"fileKey":"synthetic","rootIds":["root"],"nodes":[
        {"id":"root","type":"FRAME","fields":{"width":200,"height":100,"layoutMode":"HORIZONTAL","childrenIds":["wrapper","label"],"effects":[{"type":"LAYER_BLUR","visible":true,"radius":2}]}},
        {"id":"wrapper","type":"FRAME","fields":{"name":"Picture wrapper","parentId":"root","width":40,"height":40,"childrenIds":["picture"],"effects":[{"type":"LAYER_BLUR","visible":true,"radius":12},shadow]}},
        {"id":"picture","type":"RECTANGLE","fields":{"name":"Picture","parentId":"wrapper","isAsset":true,"width":40,"height":40,"fills":[{"type":"IMAGE","imageHash":"source","scaleMode":"CROP"}],"effects":[{"type":"LAYER_BLUR","visible":true,"radius":7}]}},
        {"id":"label","type":"TEXT","fields":{"parentId":"root","characters":"Caption","fontSize":14,"width":80,"height":20}}
    ]})).unwrap();
    let snapshot = merge_chunks(vec![chunk]).unwrap();
    let output = generate_component(
        &snapshot,
        "root",
        &CodegenOptions {
            asset_policies: std::collections::BTreeMap::from([(
                "wrapper".into(),
                AssetRepresentationPolicy {
                    render_mode: AssetRenderMode::Node,
                    effects_baked: Some(true),
                    ..Default::default()
                },
            )]),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(output.tsx.contains("blur(2px)"), "{}", output.tsx);
    assert!(!output.tsx.contains("blur(12px)"), "{}", output.tsx);
    assert!(!output.tsx.contains("blur(7px)"), "{}", output.tsx);
    assert!(!output.tsx.contains("boxShadow="), "{}", output.tsx);
    let map = json!(output.source_map);
    let baked = map["rasterizedAssets"].as_array().unwrap();
    assert_eq!(baked.len(), 1);
    assert_eq!(baked[0]["nodeId"], "wrapper");
    assert_eq!(baked[0]["effectsBakedIntoAsset"], true);
    assert!(map["entries"].as_array().unwrap().iter().any(|e| {
        e["nodeId"] == "wrapper"
            && e["property"] == "effects"
            && e["resolution"] == "effects-baked-into-asset"
            && e["generatedProperty"]
                .as_str()
                .is_some_and(|p| output.tsx.contains(p))
    }));
}

#[test]
fn live_layout_image_background_with_missing_gradient_stops_is_explicitly_unsupported() {
    let chunk: SnapshotChunk = serde_json::from_value(json!({"fileKey":"synthetic","rootIds":["root"],"nodes":[
        {"id":"root","type":"FRAME","fields":{"name":"Background","width":200,"height":100,"layoutMode":"VERTICAL","childrenIds":["text"],"fills":[{"type":"IMAGE","imageHash":"source","scaleMode":"CROP"},{"type":"GRADIENT_LINEAR","blendMode":"MULTIPLY","gradientStops":[]}],"effects":[{"type":"LAYER_BLUR","radius":8,"visible":true}]}},
        {"id":"text","type":"TEXT","fields":{"parentId":"root","characters":"Live text","fontSize":14,"width":80,"height":20}}
    ]})).unwrap();
    let snapshot = merge_chunks(vec![chunk]).unwrap();
    let output = generate_component(&snapshot, "root", &CodegenOptions::default()).unwrap();
    assert!(output.tsx.contains("Live text"));
    let issue = output
        .diagnostics
        .iter()
        .find(|d| d.code == "DEVUP_CODEGEN_IMAGE_COMPOSITION_UNSUPPORTED")
        .expect("native background composition must not be silently treated as supported");
    assert_eq!(issue.node_id.as_deref(), Some("root"));
    assert_eq!(
        issue.fidelity_impact(),
        devup_mcp_figma::FidelityImpact::Lossy
    );
    assert!(issue.message.contains("flatten the entire subtree"));
    assert!(
        json!(output.source_map)["rasterizedAssets"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
