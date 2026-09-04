//! Golden test for the 6th-round collection trim.
//!
//! Both node sets in the fixture were collected read-only from the *same*
//! live Figma node in the same session: `legacy` with the pre-trim semantics
//! (133-field manifest, prototype-chain walk into `extra`, no default
//! omission) and `trimmed` with the shipped 77-field manifest plus default
//! omission. Shrinking the manifest is only safe if the DevupUI converter
//! still produces byte-identical TSX from the smaller snapshot, which is
//! exactly what this pins.

use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_node};
use devup_mcp_figma::{RawNode, SnapshotChunk, merge_chunks};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Golden {
    file_key: String,
    root_id: String,
    legacy_utf8_bytes: usize,
    trimmed_utf8_bytes: usize,
    legacy: Vec<RawNode>,
    trimmed: Vec<RawNode>,
    expected_tsx: String,
}

fn golden() -> Golden {
    serde_json::from_str(include_str!("fixtures/manifest-trim-golden.json"))
        .expect("manifest trim golden fixture")
}

fn tsx(golden: &Golden, nodes: Vec<RawNode>) -> String {
    let snapshot = merge_chunks(vec![SnapshotChunk {
        file_key: golden.file_key.clone(),
        version: None,
        root_ids: vec![golden.root_id.clone()],
        nodes,
        diagnostics: Vec::new(),
    }])
    .expect("snapshot merges");
    generate_node(&snapshot, &golden.root_id, &CodegenOptions::default())
        .expect("codegen succeeds")
        .tsx
}

/// `generate_node` emits the bare JSX body; the server wraps it in the
/// component shell recorded as `expectedTsx`. Compare them whitespace-insensitively.
fn without_whitespace(value: &str) -> String {
    value.chars().filter(|c| !c.is_whitespace()).collect()
}

#[test]
fn the_trimmed_manifest_produces_the_same_tsx_as_the_full_legacy_collection() {
    let golden = golden();

    let legacy_tsx = tsx(&golden, golden.legacy.clone());
    let trimmed_tsx = tsx(&golden, golden.trimmed.clone());

    assert_eq!(
        legacy_tsx, trimmed_tsx,
        "trimming the collection manifest changed the converter's output"
    );
    assert!(
        without_whitespace(&golden.expected_tsx).contains(&without_whitespace(&trimmed_tsx)),
        "generated JSX no longer matches the end-to-end TSX recorded from the live run:\n{trimmed_tsx}"
    );
}

#[test]
fn the_recorded_end_to_end_tsx_carries_every_measured_design_value() {
    // Values checked against the live Figma node: fill rgb(0, 0.36346, 0.40385)
    // -> #005D67, 20px uniform padding, 9605px width, CENTER cross-axis
    // alignment, white 48px Pretendard Bold text.
    let expected = golden().expected_tsx;
    for fragment in [
        "import { Flex, Text } from \"@devup-ui/react\";",
        "alignItems=\"center\"",
        "bg=\"#005D67\"",
        "p=\"20px\"",
        "w=\"9605px\"",
        "color=\"#FFF\"",
        "fontFamily=\"Pretendard\"",
        "fontSize=\"48px\"",
        "fontWeight=\"700\"",
        "[FR-03~06] 체험하기",
    ] {
        assert!(expected.contains(fragment), "missing {fragment}");
    }
}

#[test]
fn the_trimmed_collection_is_materially_smaller_for_the_same_node() {
    let golden = golden();
    let node_count = golden.trimmed.len();
    assert_eq!(golden.legacy.len(), node_count);

    // Measured on the real node: 23,311 -> 3,862 bytes for two nodes, i.e.
    // 11,655.5 -> 1,931 bytes per node.
    let legacy_per_node = golden.legacy_utf8_bytes / node_count;
    let trimmed_per_node = golden.trimmed_utf8_bytes / node_count;
    assert!(
        trimmed_per_node * 4 < legacy_per_node,
        "expected at least a 4x reduction, got {legacy_per_node} -> {trimmed_per_node}"
    );
}

#[test]
fn no_field_the_converter_reads_was_dropped_from_the_trimmed_nodes() {
    let golden = golden();

    // Every field the trimmed collection kept must still carry the same value
    // it had under the full legacy collection - the trim may only ever remove
    // fields, never change one.
    for (legacy, trimmed) in golden.legacy.iter().zip(&golden.trimmed) {
        assert_eq!(legacy.id, trimmed.id);
        assert_eq!(legacy.node_type, trimmed.node_type);
        for (field, value) in &trimmed.fields {
            assert_eq!(
                legacy.fields.get(field),
                Some(value),
                "field {field} on node {} changed under the trim",
                trimmed.id
            );
        }
        assert!(
            trimmed.extra.is_empty(),
            "the trimmed collection must never populate `extra`"
        );
    }
}
