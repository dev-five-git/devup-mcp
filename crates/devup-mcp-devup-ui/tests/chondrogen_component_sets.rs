//! Real component sets from the chondrogen Figma file. Each one exported with
//! `componentTsx` came back unusable: the `button` set panicked in codegen, and
//! `SectionTitle` carried the sample copy, the set-canvas position of its
//! variants and `cond && "value"` attributes that evaluate to `false`.
use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;

fn load(raw: &str) -> Snapshot {
    serde_json::from_str(raw).unwrap()
}

fn button() -> Snapshot {
    load(include_str!("fixtures/chondrogen_button_set.json"))
}

fn section_title() -> Snapshot {
    load(include_str!("fixtures/chondrogen_section_title_set.json"))
}

#[test]
fn button_set_generates_without_panicking() {
    let snapshot = button();
    let root = snapshot.roots[0].clone();
    let output = std::panic::catch_unwind(|| {
        generate_component(&snapshot, &root, &CodegenOptions::default())
    })
    .expect("component codegen must not panic");
    let output = output.expect("button set must produce componentTsx");
    println!("{}", output.tsx);
    assert!(output.tsx.contains("export function"), "{}", output.tsx);
}

#[test]
fn section_title_texts_are_props_not_sample_copy() {
    let snapshot = section_title();
    let root = snapshot.roots[0].clone();
    let output = generate_component(&snapshot, &root, &CodegenOptions::default()).unwrap();
    println!("{}", output.tsx);
    let body = output.tsx.split("return (").nth(1).unwrap();
    assert!(!body.contains("CEO MESSEAGE"), "{}", output.tsx);
    assert!(!body.contains("관절재생으로 새로운 삶을"), "{}", output.tsx);
    assert!(body.contains("{ceoMesseage}"), "{}", output.tsx);
    assert!(body.contains("{text1}"), "{}", output.tsx);
    assert!(output.tsx.contains("ceoMesseage = \"CEO MESSEAGE\""));
    assert!(output.tsx.contains("text1 = \"관절재생으로 새로운 삶을\""));
}

#[test]
fn section_title_variant_roots_drop_set_canvas_placement() {
    let snapshot = section_title();
    let root = snapshot.roots[0].clone();
    let output = generate_component(&snapshot, &root, &CodegenOptions::default()).unwrap();
    assert!(!output.tsx.contains("pos=\"absolute\""), "{}", output.tsx);
    assert!(!output.tsx.contains("top={"), "{}", output.tsx);
    assert!(!output.tsx.contains("left=\"20px\""), "{}", output.tsx);
}

#[test]
fn conditional_variant_values_never_evaluate_to_false() {
    for snapshot in [section_title(), button()] {
        let root = snapshot.roots[0].clone();
        let Ok(output) = std::panic::catch_unwind(|| {
            generate_component(&snapshot, &root, &CodegenOptions::default())
        }) else {
            continue;
        };
        let Ok(output) = output else { continue };
        assert!(!output.tsx.contains(" && \""), "{}", output.tsx);
        assert!(!output.tsx.contains(" && '"), "{}", output.tsx);
    }
}

#[test]
fn named_text_props_are_unique_identifiers_with_escaped_defaults() {
    for (first, second, expected) in [
        ("Title label", "Title-label", ["titleLabel", "titleLabel2"]),
        ("align", "class", ["align2", "class2"]),
        ("", "제목", ["text1", "text2"]),
    ] {
        let mut snapshot = section_title();
        for node in snapshot
            .nodes
            .values_mut()
            .filter(|node| node.node_type == "TEXT")
        {
            let name = if node.fields["characters"] == "CEO MESSEAGE" {
                first
            } else {
                second
            };
            node.fields.insert("name".into(), serde_json::json!(name));
            node.fields.insert(
                "characters".into(),
                serde_json::json!("A \"quote\"\n{body}"),
            );
        }
        let output =
            generate_component(&snapshot, &snapshot.roots[0], &CodegenOptions::default()).unwrap();
        for name in expected {
            assert!(
                output.tsx.contains(&format!("{name}?: string")),
                "{}",
                output.tsx
            );
            assert!(
                output.tsx.contains(&format!("{{{name}}}")),
                "{}",
                output.tsx
            );
        }
        assert!(
            output.tsx.contains(r#"= "A \"quote\"\n{body}""#),
            "{}",
            output.tsx
        );
    }
}

#[test]
fn variant_root_remains_a_containing_block_for_absolute_descendants() {
    let mut snapshot = section_title();
    for node in snapshot
        .nodes
        .values_mut()
        .filter(|node| node.node_type == "TEXT")
    {
        node.fields
            .insert("layoutPositioning".into(), serde_json::json!("ABSOLUTE"));
        node.fields.insert("x".into(), serde_json::json!(3));
        node.fields.insert("y".into(), serde_json::json!(7));
    }
    let output =
        generate_component(&snapshot, &snapshot.roots[0], &CodegenOptions::default()).unwrap();
    let root = output.tsx.split("<Text").next().unwrap();
    assert!(root.contains("pos=\"relative\""), "{}", output.tsx);
    assert!(output.tsx.contains("pos=\"absolute\""), "{}", output.tsx);
    assert!(output.tsx.contains("left=\"3px\""), "{}", output.tsx);
}
