use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;
use serde_json::json;

fn fixture() -> Snapshot {
    serde_json::from_str(include_str!("fixtures/vision-wrapping-text.json")).unwrap()
}

fn render(snapshot: &Snapshot, id: &str, options: &CodegenOptions) -> String {
    generate_component(snapshot, id, options).unwrap().tsx
}

#[test]
fn captured_vision_height_text_preserves_korean_word_boundaries_and_provenance() {
    let snapshot = fixture();
    for (id, name, width, height) in [
        ("I44:3228;44:3621", "MissionWrap", 240, 46),
        ("I44:3238;44:3704", "ValueWrap", 260, 52),
    ] {
        let options = CodegenOptions {
            component_name: Some(name.into()),
            ..Default::default()
        };
        let output = generate_component(&snapshot, id, &options).unwrap();
        assert!(
            output.tsx.contains("wordBreak=\"keep-all\""),
            "{}",
            output.tsx
        );
        assert!(
            !output.tsx.contains(&format!("h=\"{height}px\"")),
            "No captured text height may be forced"
        );
        assert_eq!(snapshot.nodes[id].fields["width"], width);
        assert!(
            output
                .source_map
                .entries
                .iter()
                .any(
                    |entry| entry.generated_property.as_deref() == Some("wordBreak=\"keep-all\"")
                        && entry.property.as_deref() == Some("width")
                ),
            "{:?}",
            output.source_map
        );
        if let Ok(directory) = std::env::var("DEVUP_VISION_WRAP_PROBE_OUTPUT") {
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(
                std::path::Path::new(&directory).join(format!("{name}.tsx")),
                output.tsx,
            )
            .unwrap();
        }
    }
}

#[test]
fn unbounded_no_wrap_clamped_and_unknown_text_keep_the_existing_policy() {
    for changes in [
        json!({"width":null}),
        json!({"width":{"$unsupported":"unknown"}}),
        json!({"layoutSizingHorizontal":"HUG"}),
        json!({"layoutSizingHorizontal":null}),
        json!({"textAutoResize":"WIDTH_AND_HEIGHT"}),
        json!({"textAutoResize":"NONE"}),
        json!({"textAutoResize":null}),
        json!({"textTruncation":null}),
        json!({"textTruncation":"ENDING"}),
        json!({"maxLines":1}),
        json!({"maxLines":2}),
        json!({"maxLines":{"$unsupported":"unreadable"}}),
    ] {
        let mut snapshot = fixture();
        snapshot
            .nodes
            .get_mut("I44:3228;44:3621")
            .unwrap()
            .fields
            .extend(changes.as_object().unwrap().clone());
        let source = render(&snapshot, "I44:3228;44:3621", &CodegenOptions::default());
        assert!(
            !source.contains("wordBreak=\"normal\""),
            "{changes}: {source}"
        );
    }
    let mut snapshot = fixture();
    snapshot
        .nodes
        .get_mut("I44:3228;44:3621")
        .unwrap()
        .field_errors
        .insert("maxLines".into(), "read failed".into());
    let source = render(&snapshot, "I44:3228;44:3621", &CodegenOptions::default());
    assert!(source.contains("wordBreak=\"keep-all\""), "{source}");
}

#[test]
fn latin_text_gets_no_korean_default() {
    let mut snapshot = fixture();
    let fields = &mut snapshot.nodes.get_mut("I44:3228;44:3621").unwrap().fields;
    fields.insert("characters".into(), json!("A captured line of Latin text"));
    fields.insert(
        "styledTextSegments".into(),
        json!([{"characters":"A captured line of Latin text"}]),
    );
    let source = render(&snapshot, "I44:3228;44:3621", &CodegenOptions::default());
    assert!(!source.contains("wordBreak="), "{source}");
}

#[test]
fn korean_plain_text_without_styled_segments_keeps_word_boundaries() {
    let mut snapshot = fixture();
    let fields = &mut snapshot.nodes.get_mut("I44:3228;44:3621").unwrap().fields;
    fields.remove("styledTextSegments");
    fields.insert("characters".into(), json!("오르고 앉는 일상을 지킵니다."));
    let source = render(&snapshot, "I44:3228;44:3621", &CodegenOptions::default());
    assert!(source.contains("wordBreak=\"keep-all\""), "{source}");
    assert!(source.contains("overflowWrap=\"break-word\""), "{source}");
    assert!(!source.contains("wordBreak=\"normal\""), "{source}");
}

#[test]
fn explicit_user_typography_word_break_is_preserved_including_responsive_modes() {
    for typography in [
        json!({"wordBreak":"keep-all"}),
        json!([{"wordBreak":"break-all"},null,{"wordBreak":"keep-all"}]),
    ] {
        let mut snapshot = fixture();
        let fields = &mut snapshot.nodes.get_mut("I44:3228;44:3621").unwrap().fields;
        fields.insert("textStyleId".into(), json!("style"));
        fields["styledTextSegments"][0]["textStyleId"] = json!("style");
        let options = CodegenOptions {
            text_style_tokens: [("style".into(), "body".into())].into(),
            typography_values: [("body".into(), typography)].into(),
            ..Default::default()
        };
        let source = render(&snapshot, "I44:3228;44:3621", &options);
        assert!(source.contains("typography=\"body\""), "{source}");
        assert!(!source.contains("wordBreak="), "{source}");
    }
}
