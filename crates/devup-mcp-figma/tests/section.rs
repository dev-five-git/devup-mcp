use std::collections::BTreeMap;

use devup_mcp_figma::{
    BatchLimits, ExploreBounds, FigmaTarget, RawNode, SectionCandidate, SectionIndex,
    SectionSummary, Snapshot, build_section_index, plan_batches,
};
use serde_json::{Map, json};

#[test]
fn index_contains_only_top_level_visible_screens_in_visual_order() -> anyhow::Result<()> {
    let target =
        FigmaTarget::parse("https://www.figma.com/design/FileKey123/Fixture?node-id=10-1")?;
    let snapshot = fixture_snapshot();

    let index: SectionIndex = build_section_index(&snapshot, &target)?;

    assert_eq!(index.section.node_id, "10:1");
    assert_eq!(index.candidates.len(), 3);
    assert_eq!(
        index
            .candidates
            .iter()
            .map(|candidate| candidate.node_id.as_str())
            .collect::<Vec<_>>(),
        ["10:3", "10:2", "10:4"]
    );
    let first = &index.candidates[0];
    assert_eq!(first.name, "First");
    assert_eq!(first.node_type, "FRAME");
    assert!(first.visible);
    assert_eq!(first.direct_child_count, 1);
    assert_eq!(first.subtree_node_count, 2);
    assert!(first.estimated_serialized_bytes > 0);
    assert_eq!(first.breadcrumb, ["Proofread", "First"]);
    assert!(first.selection_reasons.contains(&"screen-like".to_owned()));
    assert!(
        first
            .selection_reasons
            .contains(&"inside-section".to_owned())
    );
    assert!(first.canonical_url.ends_with("node-id=10-3"));
    assert!(
        !index
            .candidates
            .iter()
            .any(|candidate| { matches!(candidate.node_id.as_str(), "10:5" | "10:6" | "10:7") })
    );
    Ok(())
}

#[test]
fn index_offers_small_cases_standing_beside_screen_shaped_notes() -> anyhow::Result<()> {
    // Screen shape is a guess for finding screens on an ungrouped page. A
    // Section of cases annotated with tall notes turns that guess upside down:
    // the notes measure like screens and the cases do not, so the index offered
    // every note and hid every case — an answer that looked complete.
    let target =
        FigmaTarget::parse("https://www.figma.com/design/FileKey123/Fixture?node-id=20-1")?;
    let nodes = [
        node(
            "20:1",
            "SECTION",
            json!({
                "name": "Gradient", "parentId": "0:1", "visible": true,
                "childrenIds": ["20:2", "20:3", "20:4"],
                "absoluteBoundingBox": {"x": 0, "y": 0, "width": 1600, "height": 1600}
            }),
        ),
        node(
            "20:2",
            "FRAME",
            json!({
                "name": "Code", "parentId": "20:1", "visible": true, "childrenIds": [],
                "absoluteBoundingBox": {"x": 0, "y": 300, "width": 600, "height": 391}
            }),
        ),
        node(
            "20:3",
            "FRAME",
            json!({
                "name": "Case", "parentId": "20:1", "visible": true, "childrenIds": [],
                "absoluteBoundingBox": {"x": 0, "y": 0, "width": 150, "height": 150}
            }),
        ),
        node(
            "20:4",
            "TEXT",
            json!({
                "name": "Label", "parentId": "20:1", "visible": true, "childrenIds": [],
                "characters": "Gradient", "absoluteBoundingBox": {"x": 0, "y": 700, "width": 90, "height": 24}
            }),
        ),
    ]
    .into_iter()
    .map(|node| (node.id.clone(), node))
    .collect::<BTreeMap<_, _>>();
    let snapshot = Snapshot {
        file_key: "FileKey123".to_owned(),
        version: Some("v1".to_owned()),
        roots: vec!["20:1".to_owned()],
        nodes,
        diagnostics: Vec::new(),
    };

    let index = build_section_index(&snapshot, &target)?;

    let offered = index
        .candidates
        .iter()
        .map(|candidate| candidate.node_id.as_str())
        .collect::<Vec<_>>();
    assert!(
        offered.contains(&"20:2"),
        "the note still stands: {offered:?}"
    );
    assert!(
        offered.contains(&"20:3"),
        "the case is what was asked for: {offered:?}"
    );
    assert!(
        !offered.contains(&"20:4"),
        "text on a Section labels it: {offered:?}"
    );
    Ok(())
}

#[test]
fn selection_and_batches_are_strict_bounded_and_deterministic() -> anyhow::Result<()> {
    let target =
        FigmaTarget::parse("https://www.figma.com/design/FileKey123/Fixture?node-id=10-1")?;
    let index = build_section_index(&fixture_snapshot(), &target)?;

    assert_eq!(index.select(&[], true)?, vec!["10:3", "10:2", "10:4"]);
    assert_eq!(
        index.select(&["10:4".to_owned(), "10:3".to_owned()], false)?,
        vec!["10:3", "10:4"]
    );
    assert!(
        index
            .select(&["10:3".to_owned(), "10:3".to_owned()], false)
            .is_err()
    );
    assert!(index.select(&["99:99".to_owned()], false).is_err());
    assert!(index.select(&[], false).is_err());
    assert!(index.select(&["10:3".to_owned()], true).is_err());

    let selected = index.select(&[], true)?;
    let max_nodes = index.candidates[0].subtree_node_count + index.candidates[1].subtree_node_count;
    let batches = plan_batches(
        &index,
        &selected,
        BatchLimits {
            max_estimated_bytes: usize::MAX,
            max_nodes,
        },
    )?;
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].root_ids, ["10:3", "10:2"]);
    assert_eq!(batches[1].root_ids, ["10:4"]);
    assert!(!batches.iter().any(|batch| batch.oversized));

    let oversized = plan_batches(
        &index,
        &["10:3".to_owned()],
        BatchLimits {
            max_estimated_bytes: 1,
            max_nodes: 1,
        },
    )?;
    assert_eq!(oversized.len(), 1);
    assert!(oversized[0].oversized);
    Ok(())
}

#[test]
fn packing_uses_two_balanced_batches_for_a_nontrivial_visual_sequence() -> anyhow::Result<()> {
    let index = packing_index(&[4, 4, 6, 6]);
    let selected = index
        .candidates
        .iter()
        .map(|candidate| candidate.node_id.clone())
        .collect::<Vec<_>>();

    let batches = plan_batches(
        &index,
        &selected,
        BatchLimits {
            max_estimated_bytes: 10,
            max_nodes: 10,
        },
    )?;

    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].root_ids, ["root-0", "root-2"]);
    assert_eq!(batches[1].root_ids, ["root-1", "root-3"]);
    assert!(
        batches
            .iter()
            .all(|batch| batch.estimated_bytes == 10 && batch.node_count == 10)
    );
    Ok(())
}

fn packing_index(weights: &[usize]) -> SectionIndex {
    let bounds = ExploreBounds {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
    };
    SectionIndex {
        file_key: "FileKey123".to_owned(),
        source_version: Some("v1".to_owned()),
        section: SectionSummary {
            node_id: "section".to_owned(),
            name: "Section".to_owned(),
            bounds,
        },
        candidates: weights
            .iter()
            .enumerate()
            .map(|(index, weight)| SectionCandidate {
                node_id: format!("root-{index}"),
                name: format!("Root {index}"),
                node_type: "FRAME".to_owned(),
                visible: true,
                bounds: ExploreBounds {
                    y: index as f64 * 120.0,
                    ..bounds
                },
                parent_id: Some("section".to_owned()),
                breadcrumb: vec!["Section".to_owned(), format!("Root {index}")],
                direct_child_count: 0,
                subtree_node_count: *weight,
                estimated_serialized_bytes: *weight,
                selection_reasons: vec!["screen-like".to_owned()],
                canonical_url: format!(
                    "https://www.figma.com/design/FileKey123/devup?node-id=root-{index}"
                ),
            })
            .collect(),
        truncated: false,
    }
}

fn fixture_snapshot() -> Snapshot {
    let nodes = [
        node(
            "10:1",
            "SECTION",
            json!({
                "name": "Proofread", "parentId": "0:1", "visible": true,
                "childrenIds": ["10:2", "10:3", "10:4", "10:5", "10:6"] ,
                "absoluteBoundingBox": {"x": 0, "y": 0, "width": 1600, "height": 1600}
            }),
        ),
        node(
            "10:2",
            "FRAME",
            json!({
                "name": "Second", "parentId": "10:1", "visible": true,
                "childrenIds": [], "absoluteBoundingBox": {"x": 500, "y": 100, "width": 360, "height": 740}
            }),
        ),
        node(
            "10:3",
            "FRAME",
            json!({
                "name": "First", "parentId": "10:1", "visible": true,
                "childrenIds": ["10:7"], "absoluteBoundingBox": {"x": 100, "y": 100, "width": 360, "height": 740}
            }),
        ),
        node(
            "10:4",
            "FRAME",
            json!({
                "name": "Third", "parentId": "10:1", "visible": true,
                "childrenIds": [], "absoluteBoundingBox": {"x": 100, "y": 900, "width": 360, "height": 740}
            }),
        ),
        node(
            "10:5",
            "TEXT",
            json!({
                "name": "Note", "parentId": "10:1", "visible": true, "childrenIds": [],
                "characters": "annotation", "absoluteBoundingBox": {"x": 900, "y": 100, "width": 100, "height": 30}
            }),
        ),
        node(
            "10:6",
            "FRAME",
            json!({
                "name": "Hidden", "parentId": "10:1", "visible": false,
                "childrenIds": [], "absoluteBoundingBox": {"x": 900, "y": 300, "width": 360, "height": 740}
            }),
        ),
        node(
            "10:7",
            "FRAME",
            json!({
                "name": "Nested", "parentId": "10:3", "visible": true,
                "childrenIds": [], "absoluteBoundingBox": {"x": 110, "y": 120, "width": 340, "height": 700}
            }),
        ),
    ]
    .into_iter()
    .map(|node| (node.id.clone(), node))
    .collect::<BTreeMap<_, _>>();
    Snapshot {
        file_key: "FileKey123".to_owned(),
        version: Some("v1".to_owned()),
        roots: vec!["10:1".to_owned()],
        nodes,
        diagnostics: Vec::new(),
    }
}

fn node(id: &str, node_type: &str, fields: serde_json::Value) -> RawNode {
    RawNode {
        id: id.to_owned(),
        node_type: node_type.to_owned(),
        fields: fields.as_object().cloned().unwrap_or_default(),
        extra: Map::new(),
        field_errors: BTreeMap::new(),
    }
}
