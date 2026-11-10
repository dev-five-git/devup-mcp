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

#[test]
fn html_entities_in_layer_names_do_not_leak_into_asset_paths() {
    let snapshot: Snapshot = serde_json::from_value(json!({"fileKey":"fixture","version":null,"roots":["root"],"diagnostics":[],
        "nodes":{"root":{"id":"root","type":"FRAME","fields":{"name":"Page","width":100,"height":100,"childrenIds":["img"]}},
        "img":{"id":"img","type":"RECTANGLE","fields":{"name":"R&amp;D","parentId":"root","isAsset":true,"width":40,"height":50,
            "fills":[{"type":"IMAGE","imageHash":"a","scaleMode":"CROP"}]}}}})).unwrap();
    let path = image_fill_path(&snapshot, "img", 0, false).unwrap();
    assert_eq!(path, "/images/r-d.png");
    assert!(!path.contains("amp"));
}

#[test]
fn readable_names_reuse_image_hash_across_roles_and_number_only_collisions() {
    let snapshot: Snapshot = serde_json::from_value(json!({"fileKey":"fixture","roots":["root"],"diagnostics":[],"nodes":{
        "root":{"id":"root","type":"FRAME","fields":{"name":"Main","childrenIds":["1:1","1:2","1:3"]}},
        "1:1":{"id":"1:1","type":"RECTANGLE","fields":{"name":"R&amp;D Lab","parentId":"root","isAsset":true,"fills":[{"type":"IMAGE","imageHash":"same"}]}},
        "1:2":{"id":"1:2","type":"RECTANGLE","fields":{"name":"Mobile Photo","parentId":"root","isAsset":true,"fills":[{"type":"IMAGE","imageHash":"same"}]}},
        "1:3":{"id":"1:3","type":"RECTANGLE","fields":{"name":"R&D Lab","parentId":"root","isAsset":true,"fills":[{"type":"IMAGE","imageHash":"different"}]}}
    }})).unwrap();
    let paths = ["1:1", "1:2", "1:3"].map(|id| image_fill_path(&snapshot, id, 0, true).unwrap());
    assert_eq!(
        paths,
        [
            "/images/r-d-lab.png",
            "/images/r-d-lab.png",
            "/images/r-d-lab-2.png"
        ]
    );
}

#[test]
fn distinct_svg_variants_do_not_share_a_filename_when_per_node_is_off() {
    let snapshot: Snapshot = serde_json::from_value(json!({"fileKey":"fixture","roots":["a","b"],"diagnostics":[],"nodes":{
        "a":{"id":"a","type":"VECTOR","fields":{"name":"Icon","width":24,"height":24,"isAsset":true,"variantProperties":{"Kind":"clock"}}},
        "b":{"id":"b","type":"VECTOR","fields":{"name":"Icon","width":24,"height":24,"isAsset":true,"variantProperties":{"Kind":"chart"}}}
    }})).unwrap();
    assert_ne!(
        asset_path(&snapshot, "a", false),
        asset_path(&snapshot, "b", false)
    );
}

/// Feedback 006: extra fills were `{stem}-{index}` while a different asset
/// with the same base name was deduplicated to `{stem}-2`, so fill 2 of the
/// first `hero` and the second `hero` asset both became `hero-2.png`.
#[test]
fn extra_fill_paths_cannot_collide_with_deduplicated_asset_names() {
    let snapshot: Snapshot = serde_json::from_value(json!({"fileKey":"fixture","roots":["root"],"diagnostics":[],"nodes":{
        "root":{"id":"root","type":"FRAME","fields":{"name":"Main","childrenIds":["1:1","1:2","1:3"]}},
        "1:1":{"id":"1:1","type":"RECTANGLE","fields":{"name":"Hero","parentId":"root","isAsset":true,"fills":[
            {"type":"IMAGE","imageHash":"photo-a"},{"type":"IMAGE","imageHash":"photo-b"},{"type":"IMAGE","imageHash":"photo-c"}]}},
        "1:2":{"id":"1:2","type":"RECTANGLE","fields":{"name":"Hero","parentId":"root","isAsset":true,"fills":[{"type":"IMAGE","imageHash":"photo-d"}]}},
        "1:3":{"id":"1:3","type":"RECTANGLE","fields":{"name":"Hero","parentId":"root","isAsset":true,"fills":[{"type":"IMAGE","imageHash":"photo-e"}]}}
    }})).unwrap();
    for per_node in [false, true] {
        let mut paths = Vec::new();
        for fill in 0..3 {
            paths.push(image_fill_path(&snapshot, "1:1", fill, per_node).unwrap());
        }
        paths.push(image_fill_path(&snapshot, "1:2", 0, per_node).unwrap());
        paths.push(image_fill_path(&snapshot, "1:3", 0, per_node).unwrap());
        assert_eq!(
            paths,
            [
                "/images/hero.png",
                "/images/hero--fill-1.png",
                "/images/hero--fill-2.png",
                "/images/hero-2.png",
                "/images/hero-3.png",
            ],
            "per_node={per_node}"
        );
        let unique = paths.iter().collect::<std::collections::BTreeSet<_>>();
        assert_eq!(unique.len(), paths.len(), "{paths:?}");
        // The code refers to exactly the paths the manifest reports.
        let tsx = generate_component(
            &snapshot,
            "root",
            &CodegenOptions {
                asset_names_per_node: per_node,
                ..Default::default()
            },
        )
        .unwrap()
        .tsx;
        for path in &paths[1..3] {
            assert!(tsx.contains(path.as_str()), "{path}: {tsx}");
        }
    }
}
