use std::collections::BTreeSet;

use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::{
    CollectedPayload, CollectionScope, CollectionStats, FigmaTarget, PayloadCompleteness, Snapshot,
    SnapshotChunk, UpstreamResult, collect_used_resource_refs,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SectionFixture {
    source: SectionSource,
    section: SectionNode,
    screen_candidates: Vec<ScreenCandidate>,
    proofread_target_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SectionSource {
    file_key: String,
    node_id: String,
    capture: String,
}

#[derive(Debug, Deserialize)]
struct SectionNode {
    id: String,
    name: String,
    width: f64,
    height: f64,
}

#[derive(Debug, Deserialize)]
struct ScreenCandidate {
    id: String,
    name: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FrameFixture {
    source: FrameSource,
    snapshot: Snapshot,
    resources: UpstreamResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FrameSource {
    file_key: String,
    node_id: String,
    capture: String,
    node_count: usize,
}

#[test]
fn actual_section_lists_exactly_ten_screens_in_visual_order() {
    let fixture = section_fixture();
    assert_eq!(fixture.source.file_key, "85CgSws3o5XsLv7aAwWJyS");
    assert_eq!(fixture.source.node_id, "4217:7743");
    assert_eq!(fixture.section.id, fixture.source.node_id);
    assert_eq!(fixture.section.name, "[FR-026] 본연체");
    assert_eq!(
        (fixture.section.width, fixture.section.height),
        (4589.0, 2429.0)
    );
    assert_eq!(fixture.proofread_target_id, "3879:35518");
    assert_eq!(fixture.screen_candidates.len(), 10);
    assert!(fixture.source.capture.contains("read-only"));

    let ids = fixture
        .screen_candidates
        .iter()
        .map(|candidate| candidate.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        [
            "3879:36108",
            "3879:36059",
            "3879:35503",
            "3879:35518",
            "3879:35569",
            "3879:36144",
            "3879:35729",
            "3879:35887",
            "3879:35973",
            "3879:35652",
        ]
    );
    assert!(
        fixture
            .screen_candidates
            .windows(2)
            .all(|pair| { (pair[0].y, pair[0].x) <= (pair[1].y, pair[1].x) })
    );
    assert!(fixture.screen_candidates.iter().all(|candidate| {
        candidate.width == 360.0 && candidate.height == 740.0 && !candidate.name.is_empty()
    }));
}

#[test]
fn every_actual_frame_is_reachable_and_preserves_text_and_resource_ids() {
    for candidate in section_fixture().screen_candidates {
        let fixture = frame_fixture(&candidate.id);
        assert_eq!(fixture.source.file_key, "85CgSws3o5XsLv7aAwWJyS");
        assert_eq!(fixture.source.node_id, candidate.id);
        assert!(fixture.source.capture.contains("read-only"));
        assert_eq!(fixture.snapshot.roots, [candidate.id.as_str()]);
        assert_eq!(fixture.snapshot.nodes.len(), fixture.source.node_count);
        assert!(fixture.snapshot.diagnostics.is_empty());

        for node in fixture.snapshot.nodes.values() {
            for child_id in node.typed_view().child_ids() {
                assert!(
                    fixture.snapshot.nodes.contains_key(child_id),
                    "{} references missing child {child_id}",
                    node.id
                );
            }
        }

        let text_nodes = fixture
            .snapshot
            .nodes
            .values()
            .filter(|node| node.node_type == "TEXT")
            .collect::<Vec<_>>();
        assert!(!text_nodes.is_empty(), "{} has no text", candidate.id);
        assert!(text_nodes.iter().all(|node| {
            node.typed_view()
                .value("styledTextSegments")
                .and_then(Value::as_array)
                .is_some_and(|segments| !segments.is_empty())
        }));

        let refs = collect_used_resource_refs(&[SnapshotChunk {
            file_key: fixture.snapshot.file_key.clone(),
            version: fixture.snapshot.version.clone(),
            root_ids: fixture.snapshot.roots.clone(),
            nodes: fixture.snapshot.nodes.values().cloned().collect(),
            diagnostics: fixture.snapshot.diagnostics.clone(),
        }]);
        let raw = &fixture.resources.raw;
        let variable_ids = resource_ids(raw.get("variables"));
        let style_ids = resource_ids(raw.get("styles"));
        let unresolved_ids = resource_ids(raw.get("unresolved"));
        assert!(
            refs.variable_ids
                .iter()
                .all(|id| variable_ids.contains(id) || unresolved_ids.contains(id)),
            "{} has unresolved variable references",
            candidate.id
        );
        assert!(
            refs.styles
                .iter()
                .all(|style| style_ids.contains(&style.id) || unresolved_ids.contains(&style.id)),
            "{} has unresolved style references",
            candidate.id
        );
    }
}

#[test]
fn every_actual_frame_generates_reviewed_devup_ui() {
    for candidate in section_fixture().screen_candidates {
        let fixture = frame_fixture(&candidate.id);
        let payload = CollectedPayload {
            target: FigmaTarget {
                file_key: fixture.source.file_key,
                node_id: Some(fixture.source.node_id.clone()),
                branch_key: None,
            },
            scope: CollectionScope::Node,
            metadata: Value::Null,
            snapshot: fixture.snapshot,
            variables: Some(fixture.resources.clone()),
            styles: Some(fixture.resources),
            completeness: PayloadCompleteness::UsedTokens,
            source_version: None,
            stats: CollectionStats::default(),
            assets: Vec::new(),
        };
        let output = generate_component(
            &payload.snapshot,
            &fixture.source.node_id,
            &CodegenOptions {
                component_name: Some(format!("Wquw151Frame{}", candidate.id.replace(':', ""))),
                include_diagnostics: true,
                inline_instances: true,
                ..CodegenOptions::default()
            }
            .with_payload_tokens(&payload),
        )
        .unwrap_or_else(|error| panic!("{} codegen failed: {error}", candidate.id));

        assert!(output.tsx.contains("@devup-ui/react"));
        assert!(output.tsx.contains("typography="));
        assert!(
            !output.tsx.contains(" && "),
            "{} leaked an unresolved component property",
            candidate.id
        );
        assert!(output.source_map.entries.iter().any(|entry| {
            entry.node_id.as_deref() == Some(candidate.id.as_str())
                && entry.property.as_deref() == Some("type")
        }));
        insta::assert_snapshot!(
            format!("wquw_151_frame_{}", candidate.id.replace(':', "_")),
            output.tsx
        );
    }
}

fn section_fixture() -> SectionFixture {
    serde_json::from_str(include_str!(
        "../../devup-mcp/tests/fixtures/wquw-151-section.json"
    ))
    .expect("actual WQUW-151 Section fixture")
}

fn frame_fixture(node_id: &str) -> FrameFixture {
    let source = match node_id {
        "3879:35503" => include_str!("fixtures/wquw-151-frames/3879-35503.json"),
        "3879:35518" => include_str!("fixtures/wquw-151-frames/3879-35518.json"),
        "3879:35569" => include_str!("fixtures/wquw-151-frames/3879-35569.json"),
        "3879:35652" => include_str!("fixtures/wquw-151-frames/3879-35652.json"),
        "3879:35729" => include_str!("fixtures/wquw-151-frames/3879-35729.json"),
        "3879:35887" => include_str!("fixtures/wquw-151-frames/3879-35887.json"),
        "3879:35973" => include_str!("fixtures/wquw-151-frames/3879-35973.json"),
        "3879:36059" => include_str!("fixtures/wquw-151-frames/3879-36059.json"),
        "3879:36108" => include_str!("fixtures/wquw-151-frames/3879-36108.json"),
        "3879:36144" => include_str!("fixtures/wquw-151-frames/3879-36144.json"),
        _ => panic!("unexpected WQUW-151 frame {node_id}"),
    };
    serde_json::from_str(source).expect("actual WQUW-151 frame fixture")
}

fn resource_ids(value: Option<&Value>) -> BTreeSet<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| value.get("id").and_then(Value::as_str))
        .map(str::to_owned)
        .collect()
}
