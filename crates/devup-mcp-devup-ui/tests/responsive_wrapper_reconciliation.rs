use devup_mcp_devup_ui::codegen::{
    CodegenOptions,
    responsive::{MergedScreen, merge_confirmed_breakpoints},
};
use devup_mcp_figma::Snapshot;
use serde_json::json;

// The patent page's heading is a direct mobile child and a child of a desktop
// title/summary row. The title and statistic content have identical semantics.
fn fixture() -> Snapshot {
    serde_json::from_str(include_str!(
        "fixtures/responsive-title-summary-wrappers.json"
    ))
    .unwrap()
}

fn render(source: &Snapshot) -> MergedScreen {
    merge_confirmed_breakpoints(
        source,
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap()
}

fn compact(source: &str) -> String {
    source.split_whitespace().collect()
}

#[test]
fn mobile_direct_heading_and_desktop_group_render_identical_content_once() {
    let result = render(&fixture());
    let tsx = compact(&result.tsx);
    for text in [
        "PATENTS",
        "특허·지식재산권",
        "전체3건/등록2건/출원1건",
        "특허목록",
    ] {
        assert_eq!(tsx.matches(text).count(), 1, "{text}: {}", result.tsx);
    }
    assert!(tsx.find("PATENTS").unwrap() < tsx.find("전체3건").unwrap());
    assert!(tsx.find("전체3건").unwrap() < tsx.find("특허목록").unwrap());
    assert!(
        tsx.contains("display={[\"contents\",null,null,null,\"flex\"]}"),
        "{}",
        result.tsx
    );
    assert!(
        tsx.contains("gap={[\"24px\",null,null,null,\"32px\"]}"),
        "{}",
        result.tsx
    );
    assert!(
        tsx.contains("gap={[\"8px\",null,null,null,\"12px\"]}"),
        "{}",
        result.tsx
    );
    assert!(
        tsx.contains("w={[\"320px\",null,null,null,\"400px\"]}"),
        "{}",
        result.tsx
    );
    assert!(tsx.contains("justifyContent="), "{}", result.tsx);
    assert!(tsx.contains("space-between"), "{}", result.tsx);
    assert!(tsx.contains("alignItems="), "{}", result.tsx);
    assert!(!tsx.contains("data-figma"));
    assert!(!tsx.contains("__devup_responsive"));
    let heading = result
        .elements
        .iter()
        .find(|element| element.ids.iter().any(|(_, id)| id == "heading:0"))
        .unwrap();
    assert_eq!(
        heading.ids,
        [(1440, "heading:4".into()), (360, "heading:0".into())]
    );
    assert!(
        result
            .elements
            .iter()
            .all(|element| element.ids.iter().all(|(_, id)| !id.starts_with("__devup")))
    );
    // The added box has derived source evidence, never a fabricated mobile
    // Figma node or an assertion that a real browser verified its geometry.
    assert!(!result.verified);
    assert!(
        result
            .gaps
            .iter()
            .any(|gap| gap["reason"] == "derived-responsive-wrapper-requires-runtime-review")
    );
    assert!(
        result
            .contract
            .iter()
            .any(|entry| entry["derivedTopology"].is_array())
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "DEVUP_CODEGEN_RESPONSIVE_WRAPPER_RECONCILED")
    );
}

#[test]
fn painted_padded_and_clipped_wrappers_are_retained_with_precise_review() {
    for (name, field, value) in [
        (
            "paint",
            "fills",
            json!([{"type":"SOLID","color":{"r":1,"g":0,"b":0}}]),
        ),
        ("padding", "paddingLeft", json!(24)),
        ("clipping", "clipsContent", json!(true)),
    ] {
        let mut source = fixture();
        source
            .nodes
            .get_mut("row:4")
            .unwrap()
            .fields
            .insert(field.into(), value);
        let result = render(&source);
        assert_eq!(
            result.tsx.matches("PATENTS").count(),
            2,
            "{name}: {}",
            result.tsx
        );
        assert!(!result.tsx.contains("contents"), "{name}: {}", result.tsx);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "DEVUP_CODEGEN_RESPONSIVE_TOPOLOGY_UNREPRESENTED"),
            "{name}: {:?}",
            result.diagnostics
        );
        assert!(!result.unrepresented.is_empty(), "{name}");
    }
}

#[test]
fn mobile_precedence_does_not_publish_a_synthetic_source_node() {
    let mut source = fixture();
    source.roots.reverse();
    let result = render(&source);
    assert_eq!(result.tsx.matches("PATENTS").count(), 1);
    for entry in &result.contract {
        if let Some(id) = entry["nodeId"].as_str() {
            assert!(source.nodes.contains_key(id), "{entry}");
        }
    }
    for element in &result.elements {
        assert!(
            element
                .ids
                .iter()
                .all(|(_, id)| source.nodes.contains_key(id))
        );
    }
    assert!(
        result
            .contract
            .iter()
            .any(|entry| entry["nodeId"].is_null() && entry["derivedTopology"].is_array())
    );
}

#[test]
fn noncontiguous_or_changed_content_is_not_silently_unified() {
    for case in ["noncontiguous", "outer-order", "changed", "ambiguous"] {
        let mut source = fixture();
        if case == "noncontiguous" {
            source.nodes.get_mut("page:0").unwrap().fields.insert(
                "childrenIds".into(),
                json!(["title:0", "table:0", "summary:0"]),
            );
        } else if case == "outer-order" {
            source.nodes.get_mut("page:0").unwrap().fields.insert(
                "childrenIds".into(),
                json!(["table:0", "title:0", "summary:0"]),
            );
        } else if case == "changed" {
            source
                .nodes
                .get_mut("summary-text:0")
                .unwrap()
                .fields
                .insert("characters".into(), json!("전체 4건"));
        } else {
            let mut duplicate = source.nodes["title:0"].clone();
            duplicate.id = "title:duplicate".into();
            source.nodes.insert(duplicate.id.clone(), duplicate);
            source.nodes.get_mut("page:0").unwrap().fields.insert(
                "childrenIds".into(),
                json!(["title:0", "title:duplicate", "summary:0", "table:0"]),
            );
        }
        let result = render(&source);
        assert!(!result.tsx.contains("contents"), "{case}: {}", result.tsx);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "DEVUP_CODEGEN_RESPONSIVE_TOPOLOGY_UNREPRESENTED"),
            "{case}: {:?}",
            result.diagnostics
        );
        assert!(!result.unrepresented.is_empty(), "{case}");
        if case == "changed" {
            assert!(result.tsx.contains("전체 4건"));
        }
    }
}
