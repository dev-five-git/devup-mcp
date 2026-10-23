use devup_mcp_figma::{
    AssetRenderMode, AssetSelection, Snapshot, annotate_assets, discover_asset_manifest,
    resolve_asset_selections,
};
use serde_json::json;

#[test]
fn omitted_source_format_resolves_vector_svg_and_photo_webp() {
    let snapshot: Snapshot = serde_json::from_value(json!({
        "fileKey":"defaults", "roots":["icon","photo"], "diagnostics":[], "nodes":{
            "icon":{"id":"icon","type":"VECTOR","fields":{"name":"Icon","width":24,"height":24,"fills":[{"type":"SOLID","color":{"r":0,"g":0,"b":0}}]}},
            "photo":{"id":"photo","type":"RECTANGLE","fields":{"name":"Photo","width":300,"height":200,"fills":[{"type":"IMAGE","imageHash":"photo","scaleMode":"FILL"}]}}
        }
    })).unwrap();
    for (node, extension) in [("icon", "svg"), ("photo", "webp")] {
        let selection: AssetSelection = serde_json::from_value(json!({
            "assetId":format!("{node}:node"),"scale":2
        }))
        .unwrap();
        let request = resolve_asset_selections(&snapshot, &[selection])
            .unwrap()
            .remove(0);
        assert_eq!(request.format.extension(), extension);
        assert_eq!(
            request.render_mode,
            if node == "icon" {
                AssetRenderMode::Node
            } else {
                AssetRenderMode::OriginalSource
            }
        );
    }
}

#[test]
fn hero_node_target_selects_original_photo_without_baking_text_or_buttons() {
    let snapshot: Snapshot = serde_json::from_value(json!({
        "fileKey":"hero", "roots":["hero"], "diagnostics":[], "nodes":{
            "hero":{"id":"hero","type":"FRAME","fields":{
                "name":"Hero","width":1440,"height":720,"childrenIds":["heading","buttons"],
                "fills":[{"type":"IMAGE","imageHash":"hero-photo","scaleMode":"FILL"}]
            }},
            "heading":{"id":"heading","type":"TEXT","fields":{"characters":"Build your future"}},
            "buttons":{"id":"buttons","type":"FRAME","fields":{"childrenIds":["button"]}},
            "button":{"id":"button","type":"FRAME","fields":{"name":"Button","childrenIds":["label"],"fills":[]}},
            "label":{"id":"label","type":"TEXT","fields":{"characters":"Get started"}}
        }
    })).unwrap();
    let mut selection: AssetSelection = serde_json::from_value(json!({
        "assetId":"hero:node","format":"png","scale":1
    }))
    .unwrap();
    for mode in [AssetRenderMode::OriginalSource, AssetRenderMode::Auto] {
        selection.render_mode = mode;
        let requests = resolve_asset_selections(&snapshot, &[selection.clone()]).unwrap();
        assert_eq!(requests[0].asset_id, "hero:fills:0");
        assert_eq!(requests[0].field, "fills/0");
        assert_eq!(requests[0].image_hash.as_deref(), Some("hero-photo"));
        assert_eq!(requests[0].render_mode, AssetRenderMode::OriginalSource);
    }
    selection.render_mode = AssetRenderMode::Node;
    let requests = resolve_asset_selections(&snapshot, &[selection]).unwrap();
    assert_eq!(requests[0].field, "node");
    let mut assets = discover_asset_manifest(&snapshot).assets;
    let asset = assets
        .iter_mut()
        .find(|asset| asset.node_id == "hero")
        .unwrap();
    asset.render_mode = AssetRenderMode::Node;
    annotate_assets(&snapshot, &mut assets);
    assert!(assets.iter().any(|asset| {
        asset.render_info.as_ref().unwrap()["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| {
                warning
                    .as_str()
                    .unwrap()
                    .contains("node-render-bakes-child-text")
            })
    }));
}
