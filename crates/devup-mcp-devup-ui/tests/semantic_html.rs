use devup_mcp_devup_ui::codegen::{
    CodegenOptions, generate_component, responsive::merge_breakpoints,
};
use devup_mcp_figma::Snapshot;
use serde_json::{Value, json};

fn snapshot(nodes: Value, roots: Value) -> Snapshot {
    serde_json::from_value(
        json!({"fileKey":"semantics","roots":roots,"nodes":nodes,"diagnostics":[]}),
    )
    .unwrap()
}

#[test]
fn explicit_named_regions_and_headings_keep_their_drawn_styles() {
    let nodes = json!({
        "page":{"id":"page","type":"FRAME","fields":{"name":"Screen","childrenIds":["header","main","footer"],"layoutMode":"VERTICAL"}},
        "header":{"id":"header","type":"FRAME","fields":{"name":"Header","childrenIds":["nav"],"height":80}},
        "nav":{"id":"nav","type":"FRAME","fields":{"name":"GNB","layoutMode":"HORIZONTAL"}},
        "main":{"id":"main","type":"FRAME","fields":{"name":"Main content","childrenIds":["heading","title"]}},
        "heading":{"id":"heading","type":"TEXT","fields":{"name":"Page title","characters":"Research","fontSize":48,"fontWeight":700}},
        "title":{"id":"title","type":"TEXT","fields":{"name":"Card title","characters":"A card","fontSize":48}},
        "footer":{"id":"footer","type":"FRAME","fields":{"name":"Footer","height":100}}
    });
    let output = generate_component(
        &snapshot(nodes, json!(["page"])),
        "page",
        &CodegenOptions::default(),
    )
    .unwrap();
    for tag in ["header", "nav", "main", "footer", "h1"] {
        assert_eq!(
            output.tsx.matches(&format!("as=\"{tag}\"")).count(),
            1,
            "{}",
            output.tsx
        );
    }
    assert!(output.tsx.contains("fontSize=\"48px\""), "{}", output.tsx);
    assert!(output.tsx.contains("fontWeight=\"700\""), "{}", output.tsx);
    assert!(
        !output.tsx.contains("as=\"h2\""),
        "a card title is not evidence of heading level"
    );
    assert!(
        output.diagnostics.iter().all(|entry| {
            entry.code != "DEVUP_CODEGEN_PROPERTY_UNMAPPED"
                || entry.property.as_deref() != Some("as")
        }),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn descriptive_asset_names_and_decorative_shapes_always_emit_alt() {
    for (name, alt) in [
        ("Cartilage cell model", "Cartilage cell model"),
        ("Image of cartilage", "Image of cartilage"),
        ("Rectangle-shaped model", "Rectangle-shaped model"),
        ("Image 3", ""),
        ("Rectangle 20", ""),
        ("Icon / search", ""),
    ] {
        let source = snapshot(
            json!({"image":{"id":"image","type":"RECTANGLE","fields":{
                "name":name,"width":200,"height":100,"isAsset":true,
                "fills":[{"type":"IMAGE","imageHash":"a","scaleMode":"FILL"}]
            }}}),
            json!(["image"]),
        );
        let output = generate_component(&source, "image", &CodegenOptions::default()).unwrap();
        assert!(
            output.tsx.contains(&format!("alt=\"{alt}\"")),
            "{}",
            output.tsx
        );
        assert_eq!(output.tsx.matches("alt=").count(), 1, "{}", output.tsx);
    }
}

#[test]
fn a_variant_root_uses_its_component_sets_semantic_name() {
    let source = snapshot(
        json!({
            "set":{"id":"set","type":"COMPONENT_SET","fields":{"name":"Header","childrenIds":["header"]}},
            "header":{"id":"header","type":"COMPONENT","fields":{"name":"Viewport=desktop","parentId":"set","width":1440,"height":80}}
        }),
        json!(["header"]),
    );
    let output = generate_component(&source, "header", &CodegenOptions::default()).unwrap();
    assert!(output.tsx.contains("as=\"header\""), "{}", output.tsx);
}

#[test]
fn responsive_trees_preserve_semantics_and_image_alternatives() {
    let mut nodes = serde_json::Map::new();
    for (suffix, width) in [("mobile", 360), ("desktop", 1440)] {
        let root = format!("root-{suffix}");
        let heading = format!("heading-{suffix}");
        let image = format!("image-{suffix}");
        for node in [
            json!({"id":root,"type":"FRAME","fields":{"name":"Main content","width":width,"height":500,"layoutMode":"VERTICAL","childrenIds":[heading,image]}}),
            json!({"id":heading,"type":"TEXT","fields":{"name":"Page title","parentId":root,"characters":"Research","fontSize":48}}),
            json!({"id":image,"type":"RECTANGLE","fields":{"name":"Cartilage model","parentId":root,"width":200,"height":100,"isAsset":true,"fills":[{"type":"IMAGE","imageHash":"model","scaleMode":"FILL"}]}}),
        ] {
            nodes.insert(node["id"].as_str().unwrap().into(), node);
        }
    }
    let source = snapshot(Value::Object(nodes), json!(["root-mobile", "root-desktop"]));
    let merged = merge_breakpoints(&source, &CodegenOptions::default())
        .unwrap()
        .unwrap();
    for prop in ["as=\"main\"", "as=\"h1\"", "alt=\"Cartilage model\""] {
        assert!(merged.tsx.contains(prop), "missing {prop}: {}", merged.tsx);
    }
}

#[test]
fn responsive_inlining_retains_concrete_text_and_style_overrides() {
    let mut nodes = serde_json::Map::new();
    for (suffix, width, gap) in [("mobile", 360, 12), ("desktop", 1440, 24)] {
        let root = format!("root-{suffix}");
        let instance = format!("instance-{suffix}");
        let label = format!("label-{suffix}");
        let second = format!("second-{suffix}");
        for node in [
            json!({"id":root,"type":"FRAME","fields":{"name":"Screen","width":width,"height":500,"layoutMode":"VERTICAL","childrenIds":[instance]}}),
            json!({"id":instance,"type":"INSTANCE","fields":{"name":"Card","parentId":root,"width":width,"height":100,"layoutMode":"VERTICAL","inferredAutoLayout":{"layoutMode":"VERTICAL","itemSpacing":gap},"itemSpacing":gap,"childrenIds":[label,second]}}),
            json!({"id":label,"type":"TEXT","fields":{"name":"Label","parentId":instance,"characters":"Concrete instance override","fontSize":16}}),
            json!({"id":second,"type":"TEXT","fields":{"name":"Body","parentId":instance,"characters":"Body","fontSize":16}}),
        ] {
            nodes.insert(node["id"].as_str().unwrap().into(), node);
        }
    }
    let source = snapshot(Value::Object(nodes), json!(["root-mobile", "root-desktop"]));
    let kept = merge_breakpoints(&source, &CodegenOptions::default())
        .unwrap()
        .unwrap();
    assert!(kept.tsx.contains("<Card"), "{}", kept.tsx);
    let expanded = merge_breakpoints(
        &source,
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    assert!(!expanded.tsx.contains("<Card"), "{}", expanded.tsx);
    for value in ["Concrete instance override", "12px", "24px"] {
        assert!(
            expanded.tsx.contains(value),
            "missing {value}: {}",
            expanded.tsx
        );
    }
    assert!(expanded.referenced_components().is_empty());
}

#[test]
fn semantic_headings_reset_user_agent_block_margins() {
    let nodes = json!({
        "page":{"id":"page","type":"FRAME","fields":{"name":"Screen","childrenIds":["h1","h2","body"],"layoutMode":"VERTICAL"}},
        "h1":{"id":"h1","type":"TEXT","fields":{"name":"Page title","characters":"Research","fontSize":48}},
        "h2":{"id":"h2","type":"TEXT","fields":{"name":"Heading 2","characters":"Results","fontSize":40}},
        "body":{"id":"body","type":"TEXT","fields":{"name":"Body","characters":"Plain copy","fontSize":16}}
    });
    let output = generate_component(
        &snapshot(nodes, json!(["page"])),
        "page",
        &CodegenOptions::default(),
    )
    .unwrap();
    for tag in ["h1", "h2"] {
        // Formatted JSX puts props on separate lines; inspect the element's
        // whole opening-tag prop block (from `<` up to the closing `>`).
        let marker = format!("as=\"{tag}\"");
        let at = output
            .tsx
            .find(&marker)
            .unwrap_or_else(|| panic!("{}", output.tsx));
        let start = output.tsx[..at].rfind('<').unwrap();
        let end = at + output.tsx[at..].find('>').unwrap();
        let element = &output.tsx[start..end];
        assert!(element.contains("my=\"0px\""), "{tag}: {}", output.tsx);
    }
    assert_eq!(
        output.tsx.matches("my=\"0px\"").count(),
        2,
        "{}",
        output.tsx
    );
    // The reset is not a silent addition: each emitted margin carries a
    // verified source derivation from the semantic stage.
    let derived = output
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == "DEVUP_CODEGEN_PROPERTY_EVIDENCE"
                && d.details.as_ref().is_some_and(|details| {
                    details["generatedProperty"] == "my=\"0px\""
                        && details["stage"] == "semantic"
                        && details["propertyMappingVerified"] == true
                })
        })
        .count();
    assert_eq!(derived, 2, "{:?}", output.diagnostics);
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_PROPERTY_UNMAPPED"),
        "{:?}",
        output.diagnostics
    );
}
