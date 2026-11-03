use devup_mcp_devup_ui::codegen::{CodegenOptions, LengthToken, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::json;
use std::collections::BTreeMap;

fn scene() -> Snapshot {
    serde_json::from_value(json!({"fileKey":"bound-rounding","roots":["root"],"diagnostics":[],"nodes":{
        "root":{"id":"root","type":"FRAME","fields":{"name":"Probe","width":360,"height":300,"layoutMode":"VERTICAL","childrenIds":["text"]}},
        "text":{"id":"text","type":"TEXT","fields":{"name":"Caption","parentId":"root","width":320,"layoutSizingHorizontal":"FIXED","textAutoResize":"HEIGHT","characters":"Company  ·   Address","fontSize":13,"lineHeight":{"unit":"PERCENT","value":150},"boundVariables":{"fontSize":{"id":"size","type":"VARIABLE_ALIAS"}}}}
    }})).unwrap()
}

#[test]
fn bound_resolved_font_uses_dynamic_integer_advance_with_source_provenance() {
    let output = generate_component(&scene(), "root", &CodegenOptions::default()).unwrap();
    assert!(
        output
            .tsx
            .contains("lineHeight=\"round(nearest, 1.5em, 1px)\""),
        "{}",
        output.tsx
    );
    assert!(output.source_map.entries.iter().any(|e| {
        e.property.as_deref() == Some("lineHeight")
            && e.calculation
                .as_deref()
                .is_some_and(|c| c.contains("effective CSS fontSize per mode"))
    }));
    assert!(output.tsx.contains("{\"  \"}"));
    assert!(output.tsx.contains("{\"   \"}"));
}

#[test]
fn fixed_unknown_and_independently_bound_line_height_keep_their_policies() {
    for case in ["fixed", "unknown", "bound-line"] {
        let mut source = scene();
        let f = &mut source.nodes.get_mut("text").unwrap().fields;
        if case == "fixed" {
            f.remove("boundVariables");
        }
        if case == "unknown" {
            f.remove("fontSize");
        }
        if case == "bound-line" {
            f["boundVariables"]["lineHeight"] = json!({"id":"line","type":"VARIABLE_ALIAS"});
        }
        let output = generate_component(&source, "root", &CodegenOptions::default()).unwrap();
        assert!(
            output.tsx.contains(if case == "fixed" {
                "lineHeight=\"20px\""
            } else {
                "lineHeight=\"150%\""
            }),
            "{}",
            output.tsx
        );
    }
}

#[test]
fn changed_rich_text_font_restates_relative_rounded_advance() {
    let mut source = scene();
    source.nodes.get_mut("text").unwrap().fields.insert("styledTextSegments".into(),json!([
        {"characters":"first","fontSize":13,"lineHeight":{"unit":"PERCENT","value":150},"boundVariables":{"fontSize":{"id":"size","type":"VARIABLE_ALIAS"}}},
        {"characters":"second","fontSize":15,"lineHeight":{"unit":"PERCENT","value":150},"boundVariables":{"fontSize":{"id":"other","type":"VARIABLE_ALIAS"}}}
    ]));
    let output = generate_component(&source, "root", &CodegenOptions::default()).unwrap();
    assert!(
        output
            .tsx
            .matches("lineHeight=\"round(nearest, 1.5em, 1px)\"")
            .count()
            >= 2,
        "{}",
        output.tsx
    );
}

#[test]
fn captured_footer_copy_and_dynamic_metrics_survive_formatter_probe_export() {
    let captured: Snapshot = serde_json::from_str(include_str!(
        "fixtures/chondrogen_footer_ascii_spacing.json"
    ))
    .unwrap();
    let mut source = scene();
    let node = source.nodes.get_mut("text").unwrap();
    node.fields = captured.nodes["43:1195"].fields.clone();
    node.fields.insert("parentId".into(), json!("root"));
    node.fields.insert(
        "boundVariables".into(),
        json!({"fontSize":{"id":"size","type":"VARIABLE_ALIAS"}}),
    );
    node.fields.remove("styledTextSegments");
    let options = CodegenOptions {
        component_name: Some("CapturedFooterProbe".into()),
        length_tokens: BTreeMap::from([(
            "size".into(),
            LengthToken {
                token: "captionSize".into(),
                value: json!(["13px", null, null, null, "14px"]),
            },
        )]),
        variable_tokens: BTreeMap::from([("size".into(), "captionSize".into())]),
        ..Default::default()
    };
    let output = generate_component(&source, "root", &options).unwrap();
    assert!(output.tsx.contains("round(nearest, 1.5em, 1px)"));
    assert!(output.tsx.contains("{\"  \"}"));
    if let Ok(file) = std::env::var("DEVUP_FOOTER_TEXT_PROBE") {
        std::fs::write(file, &output.tsx).unwrap();
    }
    if let (Ok(snapshot_file), Ok(context_file), Ok(directory)) = (
        std::env::var("DEVUP_FULL_FOOTER_SNAPSHOT"),
        std::env::var("DEVUP_FULL_FOOTER_CONTEXT"),
        std::env::var("DEVUP_FULL_FOOTER_OUTPUT"),
    ) {
        let source: Snapshot =
            serde_json::from_slice(&std::fs::read(snapshot_file).unwrap()).unwrap();
        let context: serde_json::Value =
            serde_json::from_slice(&std::fs::read(context_file).unwrap()).unwrap();
        for (id, name) in [
            ("30:1119", "FooterDesktopProbe"),
            ("43:1187", "FooterMobileProbe"),
        ] {
            let options = CodegenOptions {
                root_layout: devup_mcp_devup_ui::codegen::RootLayout::Embedded,
                inline_instances: true,
                component_name: Some(name.into()),
                variable_tokens: serde_json::from_value(context["variables"].clone()).unwrap(),
                text_style_tokens: serde_json::from_value(context["styles"].clone()).unwrap(),
                typography_values: serde_json::from_value(context["typography"].clone()).unwrap(),
                length_tokens: context["lengths"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(id, token)| {
                        (
                            id.clone(),
                            LengthToken {
                                token: token["token"].as_str().unwrap().into(),
                                value: token["value"].clone(),
                            },
                        )
                    })
                    .collect(),
                ..Default::default()
            };
            let generated = generate_component(&source, id, &options).unwrap();
            assert!(generated.tsx.contains("round(nearest, 1.5em, 1px)"));
            std::fs::write(
                std::path::Path::new(&directory).join(format!("{name}.tsx")),
                generated.tsx,
            )
            .unwrap();
        }
    }
}
