use devup_mcp_devup_ui::codegen::{
    CodegenOptions,
    responsive::{MergedScreen, merge_confirmed_breakpoints},
};
use devup_mcp_figma::Snapshot;
use serde_json::json;
fn fixture() -> Snapshot {
    serde_json::from_str(include_str!("fixtures/news-detail-responsive-copy.json")).unwrap()
}
fn render(s: &Snapshot) -> MergedScreen {
    merge_confirmed_breakpoints(
        s,
        &CodegenOptions {
            inline_instances: true,
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap()
}
#[test]
fn actual_news_title_retains_each_captured_copy_and_viewport_style() {
    let s = fixture();
    let o = render(&s);
    assert!(
        o.tsx.contains("[기업 뉴스] 뉴스 제목이 들어갑니다"),
        "{}",
        o.tsx
    );
    assert!(o.tsx.contains("제목 영역"), "{}", o.tsx);
    assert!(
        !o.unrepresented
            .iter()
            .any(|n| n.detail.contains("reads differently")),
        "{:?}",
        o.unrepresented
    );
    for (id, width, other) in [("73:4997", 360, 1440), ("73:4972", 1440, 360)] {
        let matches = o
            .elements
            .iter()
            .filter(|e| e.ids.iter().any(|(w, n)| *w == width && n == id))
            .collect::<Vec<_>>();
        assert_eq!(matches.len(), 1);
        assert!(matches[0].hidden_at.contains(&other));
        assert!(!matches[0].ids.iter().any(|(w, _)| *w == other));
    }
    if let Ok(p) = std::env::var("DEVUP_LITERAL_TEXT_PROBE") {
        std::fs::write(p, o.module("LiteralTextProbe")).unwrap();
    }
}
#[test]
fn same_copy_stays_shared_and_line_break_only_still_uses_existing_merge() {
    for (desktop, mobile) in [
        ("Title content", "Title content"),
        ("Titlecontent", "Title\ncontent"),
    ] {
        let mut s = fixture();
        for id in ["73:4972", "73:4997"] {
            s.nodes
                .get_mut(id)
                .unwrap()
                .fields
                .remove("styledTextSegments");
        }
        s.nodes
            .get_mut("73:4972")
            .unwrap()
            .fields
            .insert("characters".into(), json!(desktop));
        s.nodes
            .get_mut("73:4997")
            .unwrap()
            .fields
            .insert("characters".into(), json!(mobile));
        let o = render(&s);
        assert!(
            !o.unrepresented
                .iter()
                .any(|n| n.detail.contains("reads differently"))
        );
        let e = o
            .elements
            .iter()
            .find(|e| e.ids.iter().any(|(_, id)| id == "73:4997"))
            .unwrap();
        assert!(
            e.ids.iter().any(|(_, id)| id == "73:4972"),
            "{:?}",
            o.elements
        );
    }
}
#[test]
fn unknown_copy_is_not_invented_and_missing_counterpart_stays_width_scoped() {
    let mut s = fixture();
    s.nodes
        .get_mut("73:4997")
        .unwrap()
        .fields
        .remove("characters");
    let o = render(&s);
    assert!(!o.tsx.contains("[기업 뉴스] 뉴스 제목이 들어갑니다"));
    let mut s = fixture();
    let parent = s.nodes["73:4997"].fields["parentId"]
        .as_str()
        .unwrap()
        .to_owned();
    s.nodes
        .get_mut(&parent)
        .unwrap()
        .fields
        .insert("childrenIds".into(), json!([]));
    let o = render(&s);
    assert!(o.tsx.contains("제목 영역"));
    assert!(!o.tsx.contains("[기업 뉴스] 뉴스 제목이 들어갑니다"));
}
