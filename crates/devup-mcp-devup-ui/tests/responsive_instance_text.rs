use devup_mcp_devup_ui::codegen::{CodegenOptions, responsive::merge_breakpoints};
use devup_mcp_figma::Snapshot;
use serde_json::json;

fn assert_tab_branches(snapshot: &Snapshot, ids: [&str; 4], labels: [&str; 2]) {
    let output = merge_breakpoints(
        snapshot,
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    assert!(
        !output
            .unrepresented
            .iter()
            .any(|note| note.detail.contains("reads differently")),
        "{:?}",
        output.unrepresented
    );
    for label in labels {
        assert_eq!(output.tsx.matches(label).count(), 2, "{}", output.tsx);
    }
    for (index, id) in ids.into_iter().enumerate() {
        let width = if index < 2 { 360 } else { 1440 };
        let other_width = if index < 2 { 1440 } else { 360 };
        let elements = output
            .elements
            .iter()
            .filter(|element| {
                element
                    .ids
                    .iter()
                    .any(|(at, node)| *at == width && node == id)
                    && !element.hidden_at.contains(&width)
            })
            .collect::<Vec<_>>();
        assert_eq!(elements.len(), 1, "{id}: {:?}", output.elements);
        assert!(
            elements[0].hidden_at.contains(&other_width),
            "{id}: {:?}",
            elements[0]
        );
        // Separate existing instance roots let a consumer bind one label/action
        // per branch, rather than sharing a parent link across differing text.
        assert!(!elements[0].ids.iter().any(|(at, _)| *at == other_width));
    }
}

#[test]
fn viewport_copy_changes_preserve_whole_tab_hit_areas() {
    let mut nodes = serde_json::Map::new();
    for (suffix, width, labels) in [
        ("mobile", 360, ["News", "Notice"]),
        ("desktop", 1440, ["Notice", "News"]),
    ] {
        let root = format!("root-{suffix}");
        let tabs = [format!("selected-{suffix}"), format!("default-{suffix}")];
        nodes.insert(root.clone(),json!({"id":root,"type":"FRAME","fields":{"name":"Screen","width":width,"height":100,"layoutMode":"HORIZONTAL","childrenIds":tabs}}));
        for (index, tab) in tabs.iter().enumerate() {
            let text = format!("text-{tab}");
            nodes.insert(tab.clone(),json!({"id":tab,"type":"INSTANCE","fields":{"name":"TabItem","parentId":root,"mainComponentId":format!("master-{index}"),"componentSetId":"tabs","layoutMode":"HORIZONTAL","height":44,"layoutSizingVertical":"FIXED","childrenIds":[text]}}));
            nodes.insert(text.clone(),json!({"id":text,"type":"TEXT","fields":{"name":"Label","parentId":tab,"characters":labels[index],"fontSize":16}}));
        }
    }
    let snapshot: Snapshot=serde_json::from_value(json!({"fileKey":"tabs","roots":["root-mobile","root-desktop"],"nodes":nodes,"diagnostics":[]})).unwrap();
    let ordinary = devup_mcp_devup_ui::codegen::generate_component(
        &snapshot,
        "root-mobile",
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(ordinary.tsx.contains("44px"));
    assert_tab_branches(
        &snapshot,
        [
            "selected-mobile",
            "default-mobile",
            "selected-desktop",
            "default-desktop",
        ],
        ["News", "Notice"],
    );
}

#[test]
fn captured_notice_keeps_mobile_news_and_desktop_notice_labels_on_their_own_tabs() {
    // Bounded original capture: two PageMenu subtrees plus their ancestors.
    // The source itself selects News on mobile, Notice on desktop. Preserve
    // that inconsistency so application wiring cannot silently rename a tab.
    let snapshot: Snapshot =
        serde_json::from_str(include_str!("fixtures/chondrogen_notice_tab_nodes.json")).unwrap();
    assert_tab_branches(
        &snapshot,
        [
            "I73:7835;44:5412;73:7833",
            "I73:7835;44:5412;73:7834",
            "I73:7865;40:643;73:7864",
            "I73:7865;40:643;73:7863",
        ],
        ["기업 뉴스", "공지사항"],
    );
}
