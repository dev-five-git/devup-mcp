use std::collections::BTreeMap;

use devup_mcp_figma::{
    ExploreKind, ExploreNode, ExploreOptions, FigmaTarget, RawNode, Snapshot, TargetKind,
    classify_explore_node, classify_target, explore_snapshot,
};
use serde_json::{Map, json};

fn raw_node(
    id: &str,
    node_type: &str,
    name: &str,
    bounds: [f64; 4],
    child_count: usize,
    text_preview: &str,
) -> RawNode {
    let [x, y, width, height] = bounds;
    RawNode {
        id: id.to_owned(),
        node_type: node_type.to_owned(),
        fields: serde_json::from_value(json!({
            "name": name,
            "parentId": "0:1",
            "childrenIds": [],
            "x": x,
            "y": y,
            "width": width,
            "height": height,
            "childCount": child_count,
            "textPreview": text_preview
        }))
        .unwrap(),
        extra: Map::new(),
        field_errors: BTreeMap::new(),
    }
}

fn projection(nodes_reversed: bool, truncated: bool) -> Snapshot {
    let mut nodes = vec![
        raw_node(
            "1:1",
            "FRAME",
            "[FR-026] 본연체",
            [0.0, 0.0, 1200.0, 80.0],
            1,
            "본연체",
        ),
        raw_node(
            "1:2",
            "FRAME",
            "A : STORY-F-PROOFREAD",
            [0.0, 120.0, 360.0, 740.0],
            12,
            "이야기가 글로 정리되었어요",
        ),
        raw_node(
            "1:3",
            "FRAME",
            "A : STORY-F-PROOFREAD",
            [400.0, 120.0, 360.0, 740.0],
            13,
            "공개 설정 나만 보기",
        ),
        raw_node(
            "1:4",
            "TEXT",
            "Annotation",
            [800.0, 140.0, 180.0, 40.0],
            0,
            "개발 참고",
        ),
        raw_node(
            "2:1",
            "FRAME",
            "[FR-027] 다음 기능",
            [0.0, 1000.0, 1200.0, 80.0],
            1,
            "다음 기능",
        ),
        raw_node(
            "2:2",
            "FRAME",
            "A : NEXT",
            [0.0, 1120.0, 360.0, 740.0],
            8,
            "다음 화면",
        ),
    ];
    if nodes_reversed {
        nodes.reverse();
    }
    let mut page = raw_node(
        "0:1",
        "PAGE",
        "Phase2 Hand-off",
        [0.0, 0.0, 3000.0, 3000.0],
        407,
        "",
    );
    page.fields.insert("parentId".to_owned(), json!(null));
    page.fields
        .insert("projectionTruncated".to_owned(), json!(truncated));
    nodes.push(page);
    Snapshot {
        file_key: "85CgSws3o5XsLv7aAwWJyS".to_owned(),
        version: None,
        roots: vec!["0:1".to_owned()],
        nodes: nodes
            .into_iter()
            .map(|node| (node.id.clone(), node))
            .collect(),
        diagnostics: Vec::new(),
    }
}

fn target(node_id: &str) -> FigmaTarget {
    FigmaTarget::parse(&format!(
        "https://www.figma.com/design/85CgSws3o5XsLv7aAwWJyS/Girok?node-id={}",
        node_id.replace(':', "-")
    ))
    .unwrap()
}

#[test]
fn classification_distinguishes_heading_screen_annotation_and_container() {
    let heading = ExploreNode::try_from(&raw_node(
        "1:1",
        "FRAME",
        "[FR-026] 본연체",
        [0.0, 0.0, 1200.0, 80.0],
        1,
        "본연체",
    ))
    .unwrap();
    let screen = ExploreNode::try_from(&raw_node(
        "1:2",
        "FRAME",
        "Screen",
        [0.0, 120.0, 360.0, 740.0],
        12,
        "화면",
    ))
    .unwrap();
    let annotation = ExploreNode::try_from(&raw_node(
        "1:3",
        "TEXT",
        "Note",
        [0.0, 120.0, 120.0, 30.0],
        0,
        "참고",
    ))
    .unwrap();
    let container = ExploreNode::try_from(&raw_node(
        "1:4",
        "SECTION",
        "Screens",
        [0.0, 120.0, 2400.0, 1800.0],
        8,
        "",
    ))
    .unwrap();

    assert_eq!(classify_explore_node(&heading), ExploreKind::Heading);
    assert_eq!(classify_explore_node(&screen), ExploreKind::Screen);
    assert_eq!(classify_explore_node(&annotation), ExploreKind::Annotation);
    assert_eq!(classify_explore_node(&container), ExploreKind::Container);
}

#[test]
fn heading_group_keeps_duplicate_states_and_stops_at_the_next_heading() {
    let result = explore_snapshot(
        &projection(false, false),
        &target("1:1"),
        &ExploreOptions { limit: 50 },
    )
    .unwrap();

    assert_eq!(result.anchor.kind, ExploreKind::Heading);
    assert_eq!(result.group.as_ref().unwrap().title, "[FR-026] 본연체");
    assert_eq!(
        result
            .candidates
            .iter()
            .map(|candidate| candidate.node.node_id.as_str())
            .collect::<Vec<_>>(),
        ["1:2", "1:3"]
    );
    assert!(result.candidates.iter().all(|candidate| {
        candidate
            .selection_reasons
            .iter()
            .any(|reason| reason == "screen-like")
            && candidate
                .selection_reasons
                .iter()
                .any(|reason| reason == "before-next-heading")
    }));
    assert!(
        !result
            .candidates
            .iter()
            .any(|candidate| candidate.node.node_id == "2:2")
    );
    assert!(result.candidates[0].canonical_url.ends_with("node-id=1-2"));
}

#[test]
fn candidate_order_is_stable_and_truncation_combines_projection_and_limit() {
    let normal = explore_snapshot(
        &projection(false, false),
        &target("1:1"),
        &ExploreOptions { limit: 1 },
    )
    .unwrap();
    let reversed = explore_snapshot(
        &projection(true, true),
        &target("1:1"),
        &ExploreOptions { limit: 1 },
    )
    .unwrap();

    assert_eq!(normal.candidates, reversed.candidates);
    assert!(normal.truncated);
    assert!(reversed.truncated);
}

#[test]
fn exact_screen_anchor_is_returned_without_semantic_guessing() {
    let result = explore_snapshot(
        &projection(false, false),
        &target("1:2"),
        &ExploreOptions { limit: 50 },
    )
    .unwrap();

    assert_eq!(result.anchor.kind, ExploreKind::Screen);
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(result.candidates[0].node.node_id, "1:2");
    assert_eq!(
        result.candidates[0].selection_reasons,
        ["exact-screen-anchor"]
    );
}

#[test]
fn target_classification_and_section_candidates_are_explicit_and_complete() {
    let mut page = raw_node("0:1", "PAGE", "Screens", [0.0, 0.0, 2400.0, 2000.0], 2, "");
    page.fields.insert("parentId".to_owned(), json!(null));
    page.fields.insert("childrenIds".to_owned(), json!(["1:1"]));
    let mut section = raw_node(
        "1:1",
        "SECTION",
        "Proofread States",
        [100.0, 100.0, 1400.0, 1000.0],
        3,
        "",
    );
    section.fields.insert("parentId".to_owned(), json!("0:1"));
    section
        .fields
        .insert("childrenIds".to_owned(), json!(["1:2", "1:3", "1:4"]));
    let mut first = raw_node(
        "1:2",
        "FRAME",
        "Default",
        [120.0, 180.0, 360.0, 740.0],
        4,
        "default state",
    );
    first.fields.insert("parentId".to_owned(), json!("1:1"));
    first.fields.insert("visible".to_owned(), json!(true));
    first.fields.insert(
        "breadcrumb".to_owned(),
        json!(["Screens", "Proofread States", "Default"]),
    );
    first.fields.insert("pageChildIndex".to_owned(), json!(0));
    let mut hidden = raw_node(
        "1:3",
        "FRAME",
        "Hidden",
        [520.0, 180.0, 360.0, 740.0],
        2,
        "hidden state",
    );
    hidden.fields.insert("parentId".to_owned(), json!("1:1"));
    hidden.fields.insert("visible".to_owned(), json!(false));
    let mut nested_container = raw_node(
        "1:4",
        "GROUP",
        "Nested",
        [920.0, 160.0, 400.0, 800.0],
        1,
        "",
    );
    nested_container
        .fields
        .insert("parentId".to_owned(), json!("1:1"));
    nested_container
        .fields
        .insert("childrenIds".to_owned(), json!(["1:5"]));
    let mut nested = raw_node(
        "1:5",
        "FRAME",
        "Nested screen",
        [940.0, 180.0, 360.0, 740.0],
        3,
        "nested state",
    );
    nested.fields.insert("parentId".to_owned(), json!("1:4"));
    nested.fields.insert("visible".to_owned(), json!(true));
    nested.fields.insert(
        "breadcrumb".to_owned(),
        json!(["Screens", "Proofread States", "Nested", "Nested screen"]),
    );
    nested.fields.insert("pageChildIndex".to_owned(), json!(0));
    let mut component = raw_node(
        "2:1",
        "COMPONENT",
        "Button",
        [1700.0, 100.0, 320.0, 480.0],
        2,
        "",
    );
    component.fields.insert("parentId".to_owned(), json!("0:1"));
    let other = raw_node(
        "2:2",
        "TEXT",
        "Note",
        [1700.0, 600.0, 200.0, 40.0],
        0,
        "note",
    );
    let snapshot = Snapshot {
        file_key: "85CgSws3o5XsLv7aAwWJyS".to_owned(),
        version: None,
        roots: vec!["0:1".to_owned()],
        nodes: [
            page,
            section,
            first,
            hidden,
            nested_container,
            nested,
            component,
            other,
        ]
        .into_iter()
        .map(|node| (node.id.clone(), node))
        .collect(),
        diagnostics: Vec::new(),
    };

    assert_eq!(
        classify_target(
            &snapshot,
            &FigmaTarget {
                file_key: snapshot.file_key.clone(),
                node_id: None,
                branch_key: None,
            }
        ),
        TargetKind::File
    );
    assert_eq!(classify_target(&snapshot, &target("0:1")), TargetKind::Page);
    assert_eq!(
        classify_target(&snapshot, &target("1:1")),
        TargetKind::Section
    );
    assert_eq!(
        classify_target(&snapshot, &target("1:2")),
        TargetKind::Screen
    );
    assert_eq!(
        classify_target(&snapshot, &target("2:1")),
        TargetKind::Component
    );
    assert_eq!(
        classify_target(&snapshot, &target("2:2")),
        TargetKind::Other
    );

    let result = explore_snapshot(&snapshot, &target("1:1"), &ExploreOptions { limit: 50 })
        .expect("section exploration");
    assert_eq!(result.target_kind, TargetKind::Section);
    assert_eq!(
        result
            .candidates
            .iter()
            .map(|candidate| candidate.node.node_id.as_str())
            .collect::<Vec<_>>(),
        ["1:2", "1:5"]
    );
    let first = &result.candidates[0];
    assert!(first.node.visible);
    assert_eq!(first.node.child_count, 4);
    assert_eq!(
        first.node.breadcrumb,
        ["Screens", "Proofread States", "Default"]
    );
    assert_eq!(first.node.page_child_index, Some(0));
    assert_eq!(first.node.bounds.width, 360.0);
    assert!(first.canonical_url.ends_with("node-id=1-2"));
}
