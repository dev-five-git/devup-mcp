use devup_mcp_devup_ui::codegen::{
    AssetRepresentationPolicy, CodegenOptions, asset_path, generate_component, image_fill_path,
};
use devup_mcp_figma::{AssetRenderMode, Snapshot, discover_asset_manifest};
use serde_json::json;

#[test]
fn raster_instances_with_same_variant_name_and_different_images_have_distinct_code_paths() {
    let mut snapshot:Snapshot=serde_json::from_value(json!({"fileKey":"fixture","version":null,"roots":["root"],"diagnostics":[],
        "nodes":{"root":{"id":"root","type":"FRAME","fields":{"name":"Gallery","width":200,"height":100,
            "layoutMode":"HORIZONTAL","childrenIds":["first","second"]}}}})).unwrap();
    for (id, hash) in [("first", "picture-a"), ("second", "picture-b")] {
        snapshot.nodes.insert(id.into(),serde_json::from_value(json!({"id":id,"type":"INSTANCE","fields":{
            "name":"Picture","parentId":"root","variantProperties":{"Style":"Default"},"width":40,"height":50,
            "childrenIds":[format!("{id}-image")]}})).unwrap());
        snapshot.nodes.insert(
            format!("{id}-image"),
            serde_json::from_value(json!({"id":format!("{id}-image"),"type":"RECTANGLE",
            "fields":{"name":"Rectangle","parentId":id,"isAsset":true,"width":40,"height":50,
                "fills":[{"type":"IMAGE","imageHash":hash,"scaleMode":"CROP"}]}}))
            .unwrap(),
        );
    }
    let manifest = discover_asset_manifest(&snapshot);
    assert_eq!(manifest.assets.len(), 2);
    for per_node in [false, true] {
        let first = asset_path(&snapshot, "first", per_node).unwrap();
        let second = asset_path(&snapshot, "second", per_node).unwrap();
        assert_ne!(first, second);
        let result = generate_component(
            &snapshot,
            "root",
            &CodegenOptions {
                asset_names_per_node: per_node,
                inline_instances: true,
                asset_policies: std::collections::BTreeMap::from([
                    (
                        "first".into(),
                        AssetRepresentationPolicy {
                            render_mode: AssetRenderMode::Node,
                            effects_baked: Some(true),
                            ..Default::default()
                        },
                    ),
                    (
                        "second".into(),
                        AssetRepresentationPolicy {
                            render_mode: AssetRenderMode::Node,
                            effects_baked: Some(true),
                            ..Default::default()
                        },
                    ),
                ]),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(result.tsx.contains(&first), "{}", result.tsx);
        assert!(result.tsx.contains(&second), "{}", result.tsx);
        let source = generate_component(
            &snapshot,
            "root",
            &CodegenOptions {
                asset_names_per_node: per_node,
                inline_instances: true,
                ..Default::default()
            },
        )
        .unwrap();
        let first_source = image_fill_path(&snapshot, "first-image", 0, per_node).unwrap();
        let second_source = image_fill_path(&snapshot, "second-image", 0, per_node).unwrap();
        assert_ne!(first_source, second_source);
        assert!(
            source.tsx.contains(&first_source) && source.tsx.contains(&second_source),
            "{}",
            source.tsx
        );
    }
}
