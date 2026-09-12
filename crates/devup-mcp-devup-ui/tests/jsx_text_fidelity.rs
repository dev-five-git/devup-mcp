use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::{Snapshot, SnapshotChunk, merge_chunks};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

// Independent JSX oracle, following Babel cleanJSXElementLiteralChild:
// https://github.com/babel/babel/blob/main/packages/babel-types/src/utils/react/cleanJSXElementLiteralChild.ts
// Each JSXText token is cleaned separately; element/expression boundaries are
// significant. This deliberately does not use the production escaping helper.
fn render_jsx_text(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines = normalized.split('\n').collect::<Vec<_>>();
    let last_nonempty = lines
        .iter()
        .rposition(|line| line.contains(|ch| ch != ' ' && ch != '\t'))
        .unwrap_or(0);
    let mut output = String::new();
    for (index, line) in lines.iter().enumerate() {
        let expanded = line.replace('\t', " ");
        let mut line = expanded.as_str();
        if index != 0 {
            line = line.trim_start_matches(' ');
        }
        if index + 1 != lines.len() {
            line = line.trim_end_matches(' ');
        }
        if !line.is_empty() {
            output.push_str(line);
            if index != last_nonempty {
                output.push(' ');
            }
        }
    }
    output
}

// Parse the generated text subtree, not source-map ranges or whitespace-stripped
// source. JSON string expressions are evaluated and <br /> paints a newline.
fn render(tsx: &str) -> String {
    let start = tsx.find("<Text").expect("text root");
    let end = tsx.rfind("</Text>").expect("text end") + "</Text>".len();
    let mut input = &tsx[start..end];
    let mut output = String::new();
    let mut seen_list_item = false;
    while !input.is_empty() {
        if input.starts_with('<') {
            let mut quoted = false;
            let end = input
                .char_indices()
                .find_map(|(index, ch)| {
                    if ch == '"' {
                        quoted = !quoted;
                    }
                    (ch == '>' && !quoted).then_some(index)
                })
                .expect("closed tag");
            if input.starts_with("<br ") {
                output.push('\n');
            }
            if input.starts_with("<li>") {
                if seen_list_item {
                    output.push('\n');
                }
                seen_list_item = true;
            }
            input = &input[end + 1..];
        } else if input.starts_with('{') {
            let mut stream = serde_json::Deserializer::from_str(&input[1..]).into_iter::<String>();
            output.push_str(
                &stream
                    .next()
                    .expect("expression")
                    .expect("string expression"),
            );
            let end = 1 + stream.byte_offset();
            assert!(input[end..].starts_with('}'));
            input = &input[end + 1..];
        } else {
            let end = input.find(['<', '{']).unwrap_or(input.len());
            output.push_str(&render_jsx_text(&input[..end]));
            input = &input[end..];
        }
    }
    output
}

fn snapshot(characters: &str, segments: Option<Value>) -> Snapshot {
    let mut fields = json!({"characters": characters, "textTruncation": "DISABLED"});
    if let Some(segments) = segments {
        fields["styledTextSegments"] = segments;
    }
    let chunk: SnapshotChunk = serde_json::from_value(json!({
        "fileKey": "text-fidelity", "rootIds": ["text"], "nodes": [{
            "id": "text", "type": "TEXT", "fields": fields,
            "extra": {}, "fieldErrors": {}
        }], "diagnostics": []
    }))
    .unwrap();
    merge_chunks(vec![chunk]).unwrap()
}

fn assert_round_trip(snapshot: &Snapshot, id: &str) -> String {
    let output = generate_component(
        snapshot,
        id,
        &CodegenOptions {
            component_name: Some("TextFidelity".into()),
            ..CodegenOptions::default()
        },
    )
    .unwrap();
    let expected = snapshot.nodes[id]
        .typed_view()
        .string("characters")
        .unwrap_or_default();
    assert_eq!(
        render(&output.tsx),
        design_breaks(expected),
        "node {id}:\n{}",
        output.tsx
    );
    // List-item separators have implicit layout provenance. Ordinary text must
    // retain its explicit, exact character mappings after emission changes.
    if !output.tsx.contains("<li>") {
        assert!(
            output.fidelity_report.text.complete(),
            "text provenance for {id}: {:#?}",
            output.fidelity_report.text
        );
    }
    output.tsx
}

// Design line separators each paint one break; CRLF is one separator. No
// spaces, tabs, NBSP or Unicode spaces are trimmed or collapsed by this oracle.
// This checks JSX child text plus explicit breaks, before CSS normal/nowrap
// collapsing. Those CSS modes both collapse spaces; that residue is diagnosed.
fn design_breaks(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace(['\r', '\u{2028}', '\u{2029}'], "\n")
}

#[test]
fn jsx_rule_oracle_distinguishes_source_lines_and_child_boundaries() {
    assert_eq!(render_jsx_text("\n  word\n  boundary\n"), "word boundary");
    assert_eq!(render_jsx_text("  a  b  "), "  a  b  ");
    assert_eq!(render_jsx_text("\r\n\ta\r\n \r\n b\r"), "a b");
    assert_eq!(
        render_jsx_text("\n \u{a0}\t中\u{3000} \n"),
        "\u{a0} 中\u{3000}"
    );
    assert_eq!(
        render("<Text>\nword\n<Text>\nboundary\n</Text>\n</Text>"),
        "wordboundary"
    );
    assert_eq!(
        render("<Text>\nword\n{\"boundary\"}\n</Text>"),
        "wordboundary"
    );
}

#[test]
fn jsx_adjacent_plain_segments_do_not_insert_a_word_boundary() {
    // Different source segments can have the same emitted typography. Joining
    // these two children with a source newline used to paint "under stand".
    for width in [360, 768, 1440] {
        let mut snapshot = snapshot(
            "We understand each other.",
            Some(json!([
                {"characters": "We under", "fontWeight": 400},
                {"characters": "stand each other.", "fontWeight": 400}
            ])),
        );
        snapshot
            .nodes
            .get_mut("text")
            .unwrap()
            .fields
            .insert("width".into(), json!(width));
        assert_round_trip(&snapshot, "text");
    }
}

#[test]
fn jsx_mixed_styled_and_plain_children_round_trip() {
    assert_round_trip(
        &snapshot(
            "강조 이해합니다끝",
            Some(json!([
                {"characters": "강조 ", "fontWeight": 700},
                {"characters": "이해", "fontWeight": 400},
                {"characters": "합니다", "fontWeight": 400},
                {"characters": "끝", "fontWeight": 700}
            ])),
        ),
        "text",
    );
    assert_round_trip(
        &snapshot(
            "\u{a0}강조 \n이해\t합니다\n끝\u{2009}",
            Some(json!([
                {"characters": "\u{a0}강조 \n", "fontWeight": 700},
                {"characters": "이해\t", "fontWeight": 400},
                {"characters": "합니다\n", "fontWeight": 400},
                {"characters": "끝\u{2009}", "fontWeight": 700}
            ])),
        ),
        "text",
    );
}

#[test]
fn jsx_already_faithful_text_keeps_its_existing_spelling() {
    for (characters, emitted) in [
        ("plain text", "plain text"),
        ("中 文", "中 文"),
        (" a  b ", "{\" \"}a  b{\" \"}"),
        ("a\nb", "a<br />b"),
        ("<&>", "{\"<&>\"}"),
    ] {
        let tsx = assert_round_trip(&snapshot(characters, None), "text");
        assert!(tsx.contains(&format!("\n      {emitted}\n")), "{tsx}");
    }
}

#[test]
fn jsx_empty_segments_do_not_reintroduce_source_line_spaces() {
    for pieces in [
        ["a", "", "b"],
        ["a", "", ""],
        ["", "", "中"],
        ["a", "\u{a0}", "中"],
    ] {
        let characters = pieces.concat();
        let segments = pieces.map(|characters| json!({"characters": characters}));
        assert_round_trip(&snapshot(&characters, Some(json!(segments))), "text");
    }
}

#[test]
fn jsx_whitespace_and_special_characters_round_trip() {
    for value in [
        "",
        " ",
        "   ",
        " leading",
        "trailing ",
        "  a   b  ",
        "\u{a0}word\u{a0}",
        "中\u{a0}文",
        "\u{3000}한글\u{3000}",
        "a\tb",
        "\ttext\t",
        "\nfirst\nlast\n",
        "\n\n",
        "a\r\nb",
        "a\rb\r",
        "\u{2028}a\u{2029}",
        "a \n  b",
        "\n\"\\{<&>}\t\n",
    ] {
        assert_round_trip(&snapshot(value, None), "text");
        assert_round_trip(
            &snapshot(value, Some(json!([{"characters": value}]))),
            "text",
        );
    }
}

#[test]
fn jsx_empty_segment_array_preserves_node_characters() {
    assert_round_trip(&snapshot("Still here", Some(json!([]))), "text");
}

#[test]
fn jsx_css_collapsing_is_reported_without_changing_whitespace_mode() {
    for nowrap in [false, true] {
        for value in [" leading", "trailing ", "a  b", "a\tb", "a \nb"] {
            let mut snapshot = snapshot(value, None);
            if nowrap {
                snapshot
                    .nodes
                    .get_mut("text")
                    .unwrap()
                    .fields
                    .insert("maxLines".into(), json!(1));
            }
            let output = generate_component(&snapshot, "text", &CodegenOptions::default()).unwrap();
            let diagnostic = output
                .diagnostics
                .iter()
                .find(|d| d.code == "DEVUP_CODEGEN_TEXT_WHITESPACE_COLLAPSE")
                .expect("CSS collapse must not be silent");
            assert_eq!(diagnostic.node_id.as_deref(), Some("text"));
            assert_eq!(diagnostic.property.as_deref(), Some("characters"));
            assert_eq!(
                diagnostic.fidelity_impact,
                Some(devup_mcp_figma::FidelityImpact::Lossy)
            );
            assert!(output.fidelity_report.impacts.lossy > 0);
            assert!(!output.fidelity_report.strict_compatible());
            assert_eq!(output.tsx.contains("whiteSpace=\"nowrap\""), nowrap);
            assert!(!output.tsx.contains("whiteSpace=\"pre"));
            assert_round_trip(&snapshot, "text");
        }
    }
    for value in ["a b", "\u{a0}word\u{2009}", "中\u{200b}文"] {
        let output =
            generate_component(&snapshot(value, None), "text", &CodegenOptions::default()).unwrap();
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|d| d.code == "DEVUP_CODEGEN_TEXT_WHITESPACE_COLLAPSE")
        );
        assert_round_trip(&snapshot(value, None), "text");
    }
}

#[test]
fn jsx_list_text_preserves_line_separators_and_edge_spaces() {
    for value in ["first\nsecond", " first \n second \n", "a\r\nb\r\n"] {
        let tsx = assert_round_trip(
            &snapshot(
                value,
                Some(json!([{
                    "characters": value, "listOptions": {"type": "UNORDERED"}
                }])),
            ),
            "text",
        );
        assert_eq!(
            tsx.matches("<li>").count(),
            2,
            "do not add a list marker for a trailing break"
        );
    }
}

fn json_files(root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            paths.extend(json_files(&path));
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            paths.push(path);
        }
    }
    paths.sort();
    paths
}

#[test]
fn jsx_existing_fixture_texts_round_trip() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut count = 0;
    let mut css_collapsing = 0;
    let mut failures = Vec::new();
    for path in json_files(&root.join("../../fixtures/devup-figma-plugin/cases"))
        .into_iter()
        .chain(json_files(&root.join("tests/fixtures/wquw-151-frames")))
    {
        let value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let raw = value
            .get("snapshot")
            .or_else(|| value.get("payload").and_then(|p| p.get("snapshot")));
        let Some(raw) = raw else { continue };
        let snapshot: Snapshot = serde_json::from_value(raw.clone()).unwrap();
        for node in snapshot
            .nodes
            .values()
            .filter(|node| node.typed_view().node_type() == "TEXT")
        {
            let output = generate_component(
                &snapshot,
                &node.id,
                &CodegenOptions {
                    component_name: Some("TextFidelity".into()),
                    ..CodegenOptions::default()
                },
            )
            .unwrap_or_else(|error| panic!("{} / {}: {error:?}", path.display(), node.id));
            let view = node.typed_view();
            let expected = view
                .string("characters")
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    view.value("styledTextSegments")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|segment| segment.get("characters").and_then(Value::as_str))
                        .collect::<String>()
                });
            let actual = render(&output.tsx);
            if output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "DEVUP_CODEGEN_TEXT_WHITESPACE_COLLAPSE")
            {
                css_collapsing += 1;
            }
            count += 1;
            if actual != design_breaks(&expected) {
                failures.push(format!(
                    "{} / {}: expected {expected:?}, painted {actual:?}",
                    path.file_name().unwrap().to_string_lossy(),
                    node.id
                ));
            }
        }
    }
    assert!(count > 400, "representative sweep shrank: {count}");
    assert_eq!(css_collapsing, 35, "measured CSS-collapse population");
    assert!(
        failures.is_empty(),
        "{count} texts, {} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
    eprintln!(
        "JSX fixture sweep: {count} text nodes round-tripped; {css_collapsing} CSS-collapse diagnostics"
    );
}
