use std::collections::BTreeMap;

use devup_mcp_devup_ui::codegen::{
    AssetPixelSize, AssetRepresentationPolicy, AssetVariant, CodegenOptions, asset_css_map,
    generate_component, validate_asset_effects,
};
use devup_mcp_figma::{AssetRenderMode, Snapshot};
use serde_json::{Value, json};

fn source() -> AssetRepresentationPolicy {
    AssetRepresentationPolicy {
        render_mode: AssetRenderMode::OriginalSource,
        effects_baked: Some(false),
        source_pixel_size: Some(AssetPixelSize {
            width: 800.,
            height: 600.,
        }),
        output_pixel_size: Some(AssetPixelSize {
            width: 800.,
            height: 600.,
        }),
        node_box: Some(AssetPixelSize {
            width: 200.,
            height: 100.,
        }),
        asset_path: Some("/images/full.webp".into()),
        ..Default::default()
    }
}
fn node(mode: &str, t: Value) -> Value {
    json!({"id":"photo","type":"RECTANGLE","fields":{"name":"Photo","width":200,"height":100,"isAsset":true,"layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED","cornerRadius":9,"opacity":0.8,"blendMode":"MULTIPLY","fills":[{"type":"IMAGE","visible":true,"imageHash":"generic-source","scaleMode":mode,"imageTransform":t}],"effects":[{"type":"LAYER_BLUR","visible":true,"radius":12},{"type":"DROP_SHADOW","visible":true,"radius":4,"spread":0,"offset":{"x":2,"y":3},"color":{"r":0,"g":0,"b":0,"a":0.5}}]}})
}
fn generate(
    value: Value,
    policy: AssetRepresentationPolicy,
) -> devup_mcp_devup_ui::codegen::CodegenOutput {
    let snapshot: Snapshot = serde_json::from_value(
        json!({"fileKey":"synthetic","roots":["photo"],"nodes":{"photo":value},"diagnostics":[]}),
    )
    .unwrap();
    generate_component(
        &snapshot,
        "photo",
        &CodegenOptions {
            asset_policies: BTreeMap::from([("photo:fills:0".into(), policy)]),
            ..Default::default()
        },
    )
    .unwrap()
}
fn close(actual: &Value, expected: f64) {
    assert!(
        (actual.as_f64().unwrap() - expected).abs() < 1e-9,
        "{actual} != {expected}"
    );
}

#[test]
fn original_source_has_one_css_effect_and_native_has_one_baked_effect() {
    let photo = node("CROP", json!([[0.5, 0, 0.25], [0, 0.5, 0.1]]));
    let output = generate(photo.clone(), source());
    assert_eq!(
        output.tsx.matches("blur(12px)").count(),
        1,
        "{}",
        output.tsx
    );
    assert_eq!(
        output.tsx.matches("boxShadow=").count(),
        1,
        "{}",
        output.tsx
    );
    assert!(output.tsx.contains("src=\"/images/full.webp\""));
    assert!(
        output.tsx.contains("width=\"800\"") && output.tsx.contains("height=\"600\""),
        "{}",
        output.tsx
    );
    assert!(output.tsx.contains("overflow=\"hidden\""));
    assert!(
        output
            .tsx
            .contains("translate(-50%, -20%) matrix(2, 0, 0, 2, 0, 0)"),
        "{}",
        output.tsx
    );
    let mapping = &output.source_map.asset_css_mappings[0];
    assert_eq!(mapping["effectsAppliedBy"], "css");
    assert_eq!(mapping["effectsBaked"], false);
    assert!(output.source_map.rasterized_assets().is_empty());
    for evidence in mapping["emittedProperties"].as_array().unwrap() {
        assert!(
            output
                .tsx
                .contains(evidence["generatedProperty"].as_str().unwrap())
        );
    }
    let native = generate(
        photo,
        AssetRepresentationPolicy {
            render_mode: AssetRenderMode::Node,
            effects_baked: Some(true),
            asset_path: Some("/images/baked.webp".into()),
            output_pixel_size: Some(AssetPixelSize {
                width: 400.,
                height: 200.,
            }),
            ..source()
        },
    );
    assert!(
        !native.tsx.contains("blur(")
            && !native.tsx.contains("boxShadow=")
            && !native.tsx.contains("borderRadius=")
            && !native.tsx.contains("opacity="),
        "{}",
        native.tsx
    );
    assert!(!native.tsx.contains("translate(-50%"));
    assert_eq!(
        native.source_map.asset_css_mappings[0]["effectsAppliedBy"],
        "asset"
    );
    assert_eq!(
        native.source_map.rasterized_assets()[0]["effectsBakedIntoAsset"],
        true
    );
}

#[test]
fn crop_coverage_intersects_the_translated_polygon_with_source_bounds() {
    let map = asset_css_map(
        &node("CROP", json!([[0.75, 0, 0.6], [0, 0.5, -0.1]])),
        "photo:fills:0",
        "unused",
        &source(),
    );
    close(&map["visibleSourceRatio"]["w"], 0.4);
    close(&map["visibleSourceRatio"]["h"], 0.4);
    close(&map["visibleSourceRatio"]["area"], 0.16);
    assert_ne!(map["visibleSourceRatio"]["area"], json!(0.75 * 0.5));
    assert_eq!(map["cropApplied"], false);
}

#[test]
fn reflection_rotation_and_shear_use_affine_bounds_not_diagonal_products() {
    for t in [
        json!([[-1, 0, 1], [0, 1, 0]]),
        json!([[0, -1, 1], [1, 0, 0]]),
    ] {
        let map = asset_css_map(&node("CROP", t), "photo:fills:0", "unused", &source());
        close(&map["visibleSourceRatio"]["area"], 1.);
        assert!(
            map["css"]["image"]["transform"]
                .as_str()
                .unwrap()
                .contains("matrix(")
        );
    }
    let map = asset_css_map(
        &node("CROP", json!([[1, 1, 0], [0, 1, 0]])),
        "photo:fills:0",
        "unused",
        &source(),
    );
    close(&map["visibleSourceRatio"]["area"], 0.5);
    close(&map["visibleSourceRatio"]["w"], 1.);
    close(&map["visibleSourceRatio"]["h"], 1.);
}

#[test]
fn fill_fit_use_source_aspect_and_ignore_inapplicable_crop_transform() {
    for (mode, fit, area) in [("FILL", "cover", 2. / 3.), ("FIT", "contain", 1.)] {
        let map = asset_css_map(
            &node(mode, json!([[0.1, 0, 0.4], [0, 0.2, 0.3]])),
            "photo:fills:0",
            "unused",
            &source(),
        );
        assert_eq!(map["css"]["image"]["objectFit"], fit);
        close(&map["visibleSourceRatio"]["area"], area);
        assert!(
            map["warnings"]
                .as_array()
                .unwrap()
                .contains(&json!("image-transform-inapplicable-to-scale-mode"))
        );
    }
}

#[test]
fn unknown_transform_masks_and_missing_pixels_are_explicit() {
    for t in [
        Value::Null,
        json!([[0, 0, 0], [0, 0, 0]]),
        json!([[1, 0], [0, 1]]),
    ] {
        let map = asset_css_map(&node("CROP", t), "photo:fills:0", "unused", &source());
        assert_eq!(map["visibleSourceRatio"]["known"], false);
        assert!(!map["unsupported"].as_array().unwrap().is_empty());
    }
    let mut masked = node("CROP", json!([[1, 0, 0], [0, 1, 0]]));
    masked["fields"]["isMask"] = json!(true);
    let map = asset_css_map(&masked, "photo:fills:0", "unused", &source());
    assert_eq!(
        map["visibleSourceRatio"]["reason"],
        "mask-aperture-unavailable"
    );
    let unknown = generate(
        node("FILL", Value::Null),
        AssetRepresentationPolicy {
            source_pixel_size: None,
            output_pixel_size: None,
            effects_baked: None,
            ..source()
        },
    );
    assert!(!unknown.tsx.contains("width=\"800\""));
    assert_eq!(
        unknown.source_map.asset_css_mappings[0]["effectsBaked"],
        Value::Null
    );
}

#[test]
fn acquired_variants_have_truthful_density_and_intrinsic_attributes() {
    let variants = vec![
        AssetVariant {
            path: "/images/full.webp".into(),
            width: 800,
            height: 600,
            density: 1.,
        },
        AssetVariant {
            path: "/images/full@2x.webp".into(),
            width: 1600,
            height: 1200,
            density: 2.,
        },
        AssetVariant {
            path: "/images/capped.webp".into(),
            width: 900,
            height: 675,
            density: 3.,
        },
    ];
    let output = generate(
        node("FIT", Value::Null),
        AssetRepresentationPolicy {
            variants,
            ..source()
        },
    );
    assert!(
        output
            .tsx
            .contains("srcSet=\"/images/full.webp 1x, /images/full@2x.webp 2x\""),
        "{}",
        output.tsx
    );
    assert!(!output.tsx.contains("/images/capped.webp"));
    assert!(output.tsx.contains("sizes=\"200px\""));
    close(
        &output.source_map.asset_css_mappings[0]["sourceEffectiveScale"]["x"],
        4.,
    );
}

#[test]
fn double_effect_validator_matches_exact_static_paths_and_ignores_none_and_ancestors() {
    let policies = BTreeMap::from([(
        "photo:fills:0".into(),
        AssetRepresentationPolicy {
            effects_baked: Some(true),
            ..source()
        },
    )]);
    assert_eq!(
        validate_asset_effects(
            "<Image src='/images/full.webp' filter='blur(12px)' />",
            &policies
        )
        .len(),
        1
    );
    assert_eq!(
        validate_asset_effects(
            "<Image src={'/images/full.webp'} filter={\"blur(12px)\"} />",
            &policies
        )
        .len(),
        1
    );
    assert_eq!(
        validate_asset_effects(
            "<Image src=\"/images/full.webp\" filter=\"blur(12px)\" />",
            &policies
        )
        .len(),
        1
    );
    for tsx in [
        "<Image src=\"/images/full.webp\" filter=\"none\" />",
        "<Image src=\"/images/full.webp.extra\" filter=\"blur(12px)\" />",
        "<Box filter=\"blur(12px)\"><Image src=\"/images/full.webp\" /></Box>",
        "<Image src=\"/images/full.webp\" filter={dynamic} />",
    ] {
        assert!(validate_asset_effects(tsx, &policies).is_empty(), "{tsx}");
    }
}

#[test]
fn visible_capture_of_a_parent_cannot_be_squeezed_into_leaf() {
    let output = generate(
        node("FILL", Value::Null),
        AssetRepresentationPolicy {
            render_mode: AssetRenderMode::Visible,
            effects_baked: Some(true),
            capture_node_id: Some("parent".into()),
            capture_box: Some(AssetPixelSize {
                width: 600.,
                height: 500.,
            }),
            ..source()
        },
    );
    assert!(!output.tsx.contains("/images/full.webp"));
    assert!(output.tsx.contains("visibility=\"hidden\""));
    assert!(output.diagnostics.iter().any(|d| {
        d.details
            .as_ref()
            .is_some_and(|d| d["reason"] == "incompatible-visible-capture")
    }));
}

#[test]
fn multiple_image_and_gradient_paints_keep_layer_order_opacity_and_live_children() {
    let snapshot:Snapshot=serde_json::from_value(json!({"fileKey":"synthetic","roots":["root"],"diagnostics":[],"nodes":{
        "root":{"id":"root","type":"FRAME","fields":{"width":240,"height":120,"layoutMode":"VERTICAL","cornerRadius":11,"childrenIds":["label"],"fills":[{"type":"IMAGE","imageHash":"one","scaleMode":"CROP","imageTransform":[[0.5,0,0.25],[0,0.5,0.1]],"opacity":0.4},{"type":"IMAGE","imageHash":"two","scaleMode":"FIT","opacity":0.7},{"type":"GRADIENT_LINEAR","blendMode":"MULTIPLY","opacity":0.5,"gradientTransform":[[1,0,0],[0,1,0]],"gradientStops":[{"position":0,"color":{"r":1,"g":0,"b":0,"a":1}},{"position":1,"color":{"r":0,"g":0,"b":1,"a":1}}]}],"effects":[{"type":"LAYER_BLUR","radius":3}]}},
        "label":{"id":"label","type":"TEXT","fields":{"parentId":"root","characters":"Live caption","fontSize":12,"width":80,"height":20}}
    }})).unwrap();
    let options = CodegenOptions {
        asset_policies: BTreeMap::from([
            (
                "root:fills:0".into(),
                AssetRepresentationPolicy {
                    asset_path: Some("/images/one.webp".into()),
                    ..source()
                },
            ),
            (
                "root:fills:1".into(),
                AssetRepresentationPolicy {
                    asset_path: Some("/images/two.webp".into()),
                    ..source()
                },
            ),
        ]),
        ..Default::default()
    };
    let output = generate_component(&snapshot, "root", &options).unwrap();
    let tsx = &output.tsx;
    assert!(tsx.find("/images/one.webp").unwrap() < tsx.find("/images/two.webp").unwrap());
    assert!(tsx.find("/images/two.webp").unwrap() < tsx.find("linear-gradient").unwrap());
    assert!(tsx.find("linear-gradient").unwrap() < tsx.find("Live caption").unwrap());
    for expected in [
        "opacity=\"0.4\"",
        "opacity=\"0.7\"",
        "mixBlendMode=\"multiply\"",
        "borderRadius=\"11px\"",
        "blur(3px)",
    ] {
        assert!(tsx.contains(expected), "{tsx}");
    }
    assert_eq!(tsx.matches("blur(3px)").count(), 1);
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_IMAGE_COMPOSITION_UNSUPPORTED")
    );
    assert_eq!(output.source_map.asset_css_mappings.len(), 2);
}

#[test]
fn same_names_with_distinct_instances_keep_their_acquired_paths() {
    let a = asset_css_map(
        &json!({"id":"instance-a/photo","fields":{"name":"Photo","parentId":"instance-a","width":200,"height":100,"fills":[{"type":"IMAGE","scaleMode":"FIT"}]}}),
        "instance-a/photo:fills:0",
        "/images/a.webp",
        &AssetRepresentationPolicy {
            asset_path: None,
            ..source()
        },
    );
    let b = asset_css_map(
        &json!({"id":"instance-b/photo","fields":{"name":"Photo","parentId":"instance-b","width":200,"height":100,"fills":[{"type":"IMAGE","scaleMode":"FIT"}]}}),
        "instance-b/photo:fills:0",
        "/images/b.webp",
        &AssetRepresentationPolicy {
            asset_path: None,
            ..source()
        },
    );
    assert_ne!(a["instanceIdentity"], b["instanceIdentity"]);
    assert_ne!(a["assetPath"], b["assetPath"]);
}

#[test]
fn crop_hug_content_keeps_measured_display_size_instead_of_source_intrinsic_size() {
    let mut photo = node("CROP", json!([[0.5, 0, 0.25], [0, 0.5, 0.1]]));
    photo["fields"]["layoutSizingHorizontal"] = json!("HUG");
    photo["fields"]["layoutSizingVertical"] = json!("HUG");
    let output = generate(photo, source());
    assert!(
        output.tsx.contains("w=\"200px\"") && output.tsx.contains("h=\"100px\""),
        "{}",
        output.tsx
    );
    assert!(output.tsx.contains("width=\"800\"") && output.tsx.contains("height=\"600\""));
    assert!(!output.tsx.contains("w=\"800px\""));
}

#[test]
fn uncropped_hug_image_uses_its_display_content_and_embedded_root_keeps_host_control() {
    let mut photo = node("FIT", Value::Null);
    photo["fields"]["layoutSizingHorizontal"] = json!("HUG");
    photo["fields"]["layoutSizingVertical"] = json!("HUG");
    let output = generate(photo.clone(), source());
    assert!(
        output.tsx.contains("w=\"200px\"") && output.tsx.contains("h=\"100px\""),
        "{}",
        output.tsx
    );
    let snapshot: Snapshot = serde_json::from_value(
        json!({"fileKey":"synthetic","roots":["photo"],"nodes":{"photo":photo},"diagnostics":[]}),
    )
    .unwrap();
    let embedded = generate_component(
        &snapshot,
        "photo",
        &CodegenOptions {
            root_layout: devup_mcp_devup_ui::codegen::RootLayout::Embedded,
            asset_policies: BTreeMap::from([("photo:fills:0".into(), source())]),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        !embedded.tsx.contains("w=\"200px\"") && !embedded.tsx.contains("h=\"100px\""),
        "{}",
        embedded.tsx
    );
}

#[test]
fn paint_opacity_and_node_opacity_are_each_applied_once() {
    for mode in ["FILL", "CROP"] {
        let mut photo = node(mode, json!([[0.5, 0, 0.25], [0, 0.5, 0.1]]));
        photo["fields"]["fills"][0]["opacity"] = json!(0.5);
        let output = generate(photo, source());
        if mode == "FILL" {
            assert!(output.tsx.contains("opacity=\"0.4\""), "{}", output.tsx);
        } else {
            assert_eq!(
                output.tsx.matches("opacity=\"0.8\"").count(),
                1,
                "{}",
                output.tsx
            );
            assert_eq!(
                output.tsx.matches("opacity=\"0.5\"").count(),
                1,
                "{}",
                output.tsx
            );
        }
    }
}

#[test]
fn projected_resolution_accounts_for_zoom_and_fractional_variant_density() {
    let policy = AssetRepresentationPolicy {
        source_pixel_size: Some(AssetPixelSize {
            width: 1000.,
            height: 500.,
        }),
        output_pixel_size: Some(AssetPixelSize {
            width: 121.,
            height: 61.,
        }),
        node_box: Some(AssetPixelSize {
            width: 120.1,
            height: 60.05,
        }),
        variants: vec![
            AssetVariant {
                path: "/images/base.webp".into(),
                width: 121,
                height: 61,
                density: 1.,
            },
            AssetVariant {
                path: "/images/large.webp".into(),
                width: 241,
                height: 121,
                density: 241. / 121.,
            },
        ],
        ..source()
    };
    let map = asset_css_map(
        &node("CROP", json!([[0.2, 0, 0.1], [0, 0.2, 0.1]])),
        "photo:fills:0",
        "unused",
        &policy,
    );
    assert!(
        map["srcSet"]
            .as_str()
            .unwrap()
            .contains("/images/large.webp")
    );
    assert!(
        map["projectedOutputEffectiveScale"]["minimum"]
            .as_f64()
            .unwrap()
            < 0.3
    );
    assert!(
        map["warnings"]
            .as_array()
            .unwrap()
            .contains(&json!("low-resolution-bitmap"))
    );
}

#[test]
fn responsive_crop_variants_keep_scalar_html_attributes_and_affine_clip() {
    let mut nodes = serde_json::Map::new();
    let mut roots = Vec::new();
    let mut policies = BTreeMap::new();
    for (index, width) in [340, 800, 1440].into_iter().enumerate() {
        let root = format!("screen-{index}");
        let image = format!("photo-{index}");
        roots.push(root.clone());
        nodes.insert(root.clone(),json!({"id":root,"type":"FRAME","fields":{"name":(["mobile","tablet","desktop"][index]),"width":width,"height":200,"layoutMode":"VERTICAL","layoutSizingHorizontal":"FIXED","layoutSizingVertical":"FIXED","childrenIds":[image]}}));
        let mut photo = node("CROP", json!([[0.5, 0, 0.25], [0, 0.5, 0.1]]));
        photo["id"] = json!(image);
        photo["fields"]["parentId"] = json!(root);
        nodes.insert(image.clone(), photo);
        policies.insert(
            format!("{image}:fills:0"),
            AssetRepresentationPolicy {
                variants: vec![AssetVariant {
                    path: format!("/images/variant-{index}.webp"),
                    width: 800,
                    height: 600,
                    density: 1.,
                }],
                ..source()
            },
        );
    }
    let snapshot: Snapshot = serde_json::from_value(
        json!({"fileKey":"synthetic","roots":roots,"nodes":nodes,"diagnostics":[]}),
    )
    .unwrap();
    let merged = devup_mcp_devup_ui::codegen::responsive::merge_breakpoints(
        &snapshot,
        &CodegenOptions {
            asset_policies: policies,
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    for name in ["src", "srcSet", "sizes", "width", "height"] {
        assert!(
            !merged.tsx.contains(&format!("{name}={{[")),
            "{}",
            merged.tsx
        );
    }
    for index in 0..3 {
        assert!(
            merged
                .tsx
                .contains(&format!("/images/variant-{index}.webp")),
            "{}",
            merged.tsx
        );
    }
    assert!(merged.tsx.contains("overflow=\"hidden\""));
    assert!(merged.tsx.contains("translate(-50%, -20%)"));
}
