use std::collections::BTreeMap;

use devup_mcp_figma::{
    ExploreKind, ExploreNode, ExploreOptions, FigmaTarget, RawNode, Snapshot, TargetKind,
    classify_explore_node, classify_target, collect_section_notes, explore_snapshot,
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
            "[FR-026] Essence",
            [0.0, 0.0, 1200.0, 80.0],
            1,
            "Essence",
        ),
        raw_node(
            "1:2",
            "FRAME",
            "A : STORY-F-PROOFREAD",
            [0.0, 120.0, 360.0, 740.0],
            12,
            "Your story has been written up",
        ),
        raw_node(
            "1:3",
            "FRAME",
            "A : STORY-F-PROOFREAD",
            [400.0, 120.0, 360.0, 740.0],
            13,
            "Visibility: only me",
        ),
        raw_node(
            "1:4",
            "TEXT",
            "Annotation",
            [800.0, 140.0, 180.0, 40.0],
            0,
            "Dev note",
        ),
        raw_node(
            "2:1",
            "FRAME",
            "[FR-027] Next feature",
            [0.0, 1000.0, 1200.0, 80.0],
            1,
            "Next feature",
        ),
        raw_node(
            "2:2",
            "FRAME",
            "A : NEXT",
            [0.0, 1120.0, 360.0, 740.0],
            8,
            "Next screen",
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

fn nested_wquw_section_projection() -> Snapshot {
    let screen_ids = [
        "3879:35518",
        "3879:35519",
        "3879:35520",
        "3879:35521",
        "3879:35522",
        "3879:35523",
        "3879:35524",
        "3879:35525",
        "3879:35526",
        "3879:35527",
    ];
    let mut page = raw_node(
        "0:1",
        "PAGE",
        "Phase2 Hand-off",
        [0.0, 0.0, 5_000.0, 1_000.0],
        1,
        "",
    );
    page.fields.insert("parentId".to_owned(), json!(null));
    page.fields
        .insert("childrenIds".to_owned(), json!(["4217:7743"]));

    let mut section = raw_node(
        "4217:7743",
        "SECTION",
        "[FR-026] Essence",
        [0.0, 0.0, 4_400.0, 900.0],
        11,
        "",
    );
    section.fields.insert("parentId".to_owned(), json!("0:1"));
    section.fields.insert(
        "childrenIds".to_owned(),
        json!(["3879:35481", "screen-wrapper"]),
    );

    let mut heading = raw_node(
        "3879:35481",
        "FRAME",
        "[FR-026] Essence",
        [0.0, 0.0, 1_200.0, 80.0],
        1,
        "Essence",
    );
    heading
        .fields
        .insert("parentId".to_owned(), json!("4217:7743"));

    let mut wrapper = raw_node(
        "screen-wrapper",
        "GROUP",
        "Screens",
        [0.0, 100.0, 4_400.0, 740.0],
        10,
        "",
    );
    wrapper
        .fields
        .insert("parentId".to_owned(), json!("4217:7743"));
    wrapper
        .fields
        .insert("childrenIds".to_owned(), json!(screen_ids));

    let mut nodes = vec![page, section, heading, wrapper];
    for (index, id) in screen_ids.into_iter().enumerate() {
        let mut screen = raw_node(
            id,
            "FRAME",
            &format!("Screen {index}"),
            [index as f64 * 400.0, 100.0, 360.0, 740.0],
            1,
            "",
        );
        screen
            .fields
            .insert("parentId".to_owned(), json!("screen-wrapper"));
        nodes.push(screen);
    }
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

#[test]
fn classification_distinguishes_heading_screen_annotation_and_container() {
    let heading = ExploreNode::try_from(&raw_node(
        "1:1",
        "FRAME",
        "[FR-026] Essence",
        [0.0, 0.0, 1200.0, 80.0],
        1,
        "Essence",
    ))
    .unwrap();
    let screen = ExploreNode::try_from(&raw_node(
        "1:2",
        "FRAME",
        "Screen",
        [0.0, 120.0, 360.0, 740.0],
        12,
        "Screen",
    ))
    .unwrap();
    let annotation = ExploreNode::try_from(&raw_node(
        "1:3",
        "TEXT",
        "Note",
        [0.0, 120.0, 120.0, 30.0],
        0,
        "Note",
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
    assert_eq!(result.group.as_ref().unwrap().title, "[FR-026] Essence");
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
fn nested_heading_uses_its_section_scope_without_replacing_the_public_anchor() {
    let snapshot = nested_wquw_section_projection();
    let heading = explore_snapshot(
        &snapshot,
        &target("3879:35481"),
        &ExploreOptions { limit: 50 },
    )
    .unwrap();
    let section = explore_snapshot(
        &snapshot,
        &target("4217:7743"),
        &ExploreOptions { limit: 50 },
    )
    .unwrap();
    let ids = |result: &devup_mcp_figma::ExploreResult| {
        result
            .candidates
            .iter()
            .map(|candidate| candidate.node.node_id.clone())
            .collect::<Vec<_>>()
    };

    assert_eq!(heading.anchor.node_id, "3879:35481");
    assert_eq!(
        heading.group.as_ref().unwrap().heading_node_id.as_deref(),
        Some("3879:35481")
    );
    assert_eq!(ids(&heading), ids(&section));
    assert_eq!(
        ids(&heading),
        [
            "3879:35518",
            "3879:35519",
            "3879:35520",
            "3879:35521",
            "3879:35522",
            "3879:35523",
            "3879:35524",
            "3879:35525",
            "3879:35526",
            "3879:35527",
        ]
        .map(str::to_owned)
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

#[test]
fn section_notes_combine_direct_text_and_descendant_annotations() {
    let mut section = raw_node(
        "1:1",
        "SECTION",
        "Documented section",
        [0.0, 0.0, 1200.0, 900.0],
        2,
        "",
    );
    section.fields.insert("parentId".to_owned(), json!(null));
    section
        .fields
        .insert("childrenIds".to_owned(), json!(["1:2", "1:3"]));
    let mut direct = raw_node("1:2", "TEXT", "Note", [0.0, 0.0, 200.0, 30.0], 0, "");
    direct.fields.insert("parentId".to_owned(), json!("1:1"));
    direct
        .fields
        .insert("characters".to_owned(), json!("  Introductory note  "));
    let mut frame = raw_node("1:3", "FRAME", "Card", [0.0, 50.0, 360.0, 740.0], 1, "");
    frame.fields.insert("parentId".to_owned(), json!("1:1"));
    frame
        .fields
        .insert("childrenIds".to_owned(), json!(["1:4"]));
    frame.fields.insert(
        "annotations".to_owned(),
        json!([{"label": " Use the compact variant "}, {"label": "   "}]),
    );
    let mut image = raw_node(
        "1:4",
        "RECTANGLE",
        "Hero image",
        [0.0, 50.0, 360.0, 200.0],
        0,
        "",
    );
    image.fields.insert("parentId".to_owned(), json!("1:3"));
    image.fields.insert(
        "annotations".to_owned(),
        json!([{"labelMarkdown": "Maintain the aspect ratio"}]),
    );
    let snapshot = Snapshot {
        file_key: "file-key".to_owned(),
        version: None,
        roots: vec!["1:1".to_owned()],
        nodes: [section, direct, frame, image]
            .into_iter()
            .map(|node| (node.id.clone(), node))
            .collect(),
        diagnostics: Vec::new(),
    };

    let expected =
        "Introductory note\n[Card] Use the compact variant\n[Hero image] Maintain the aspect ratio";
    assert_eq!(collect_section_notes(&snapshot, "1:1").unwrap(), expected);
    let explored = explore_snapshot(&snapshot, &target("1:1"), &ExploreOptions { limit: 50 })
        .expect("Section exploration");
    assert_eq!(explored.group.expect("Section group").notes, expected);
}
