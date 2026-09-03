//! Pins the exact set of node fields `fast_snapshot.js` may omit from the
//! envelope, by replaying the omission over the ten real WQUW-151 screens
//! (1,500+ nodes covering every node type the file uses) and requiring the
//! generated TSX to stay byte-identical.
//!
//! The rules here and the `SCALAR_DEFAULTS` / `NULL_SENSITIVE_FIELDS` tables in
//! `crates/devup-mcp-figma/src/scripts/fast_snapshot.js` must stay in sync;
//! this test is what makes that safe to change.
//!
//! Fields deliberately NOT omitted, each for a reason visible in the converter:
//!   - `maxWidth` / `maxHeight`: `codegen/layout.rs` compares
//!     `view.value("maxWidth") != Some(&Value::Null)`, so a present-null and an
//!     absent field take opposite branches.
//!   - `opacity`: `codegen/component.rs` locates a hover variant with
//!     `number("opacity").is_some()` - presence itself is the signal.
//!   - `visible`: the component registration snapshot emits a `"visible"` line
//!     whenever the field is present.
//!   - `layoutPositioning`: compared against `Some("AUTO")`, so absence is not
//!     equivalent to the default.
//!   - per-corner radii and per-side stroke weights: they feed shorthand
//!     builders that read the corners/sides as a group, so dropping the ones
//!     that happen to be zero would change the shorthand.

use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::{RawNode, Snapshot};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FrameFixture {
    source: FrameSource,
    snapshot: Snapshot,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FrameSource {
    node_id: String,
}

/// Mirrors `STYLE_ID_FIELDS` in `fast_snapshot.js`.
const STYLE_ID_FIELDS: &[&str] = &[
    "backgroundStyleId",
    "effectStyleId",
    "fillStyleId",
    "gridStyleId",
    "strokeStyleId",
    "textStyleId",
];

/// Mirrors `NULL_SENSITIVE_FIELDS` in `fast_snapshot.js`: fields whose
/// present-null is load-bearing and must survive the omission.
const NULL_SENSITIVE_FIELDS: &[&str] = &["maxWidth", "maxHeight"];

/// Mirrors `SCALAR_DEFAULTS` in `fast_snapshot.js`.
fn scalar_defaults() -> Vec<(&'static str, Value)> {
    use serde_json::json;
    vec![
        ("rotation", json!(0)),
        ("cornerRadius", json!(0)),
        ("isAsset", json!(false)),
        ("isMask", json!(false)),
        ("clipsContent", json!(false)),
        ("blendMode", json!("PASS_THROUGH")),
        ("strokeAlign", json!("INSIDE")),
        ("textCase", json!("ORIGINAL")),
        ("textDecoration", json!("NONE")),
        ("textAlignHorizontal", json!("LEFT")),
        ("textAlignVertical", json!("TOP")),
        ("counterAxisAlignItems", json!("MIN")),
        ("primaryAxisAlignItems", json!("MIN")),
        ("gridColumnCount", json!(0)),
        ("gridRowCount", json!(0)),
        ("gridColumnGap", json!(0)),
        ("gridRowGap", json!(0)),
        ("gridColumnAnchorIndex", json!(-1)),
        ("gridRowAnchorIndex", json!(-1)),
    ]
}

fn numbers_equal(left: &Value, right: &Value) -> bool {
    match (left.as_f64(), right.as_f64()) {
        (Some(left), Some(right)) => (left - right).abs() < f64::EPSILON,
        _ => left == right,
    }
}

fn is_omittable(field: &str, value: &Value) -> bool {
    if value.is_null() {
        return !NULL_SENSITIVE_FIELDS.contains(&field);
    }
    if value.as_array().is_some_and(Vec::is_empty) {
        return true;
    }
    if value.as_object().is_some_and(serde_json::Map::is_empty) {
        return true;
    }
    if value.as_str() == Some("") && STYLE_ID_FIELDS.contains(&field) {
        return true;
    }
    scalar_defaults()
        .iter()
        .any(|(name, default)| *name == field && numbers_equal(value, default))
}

fn omit_defaults(node: &mut RawNode) -> usize {
    let before = node.fields.len();
    node.fields
        .retain(|field, value| !is_omittable(field, value));
    node.extra.clear();
    before - node.fields.len()
}

fn fixtures() -> Vec<FrameFixture> {
    [
        include_str!("fixtures/wquw-151-frames/3879-35503.json"),
        include_str!("fixtures/wquw-151-frames/3879-35518.json"),
        include_str!("fixtures/wquw-151-frames/3879-35569.json"),
        include_str!("fixtures/wquw-151-frames/3879-35652.json"),
        include_str!("fixtures/wquw-151-frames/3879-35729.json"),
        include_str!("fixtures/wquw-151-frames/3879-35887.json"),
        include_str!("fixtures/wquw-151-frames/3879-35973.json"),
        include_str!("fixtures/wquw-151-frames/3879-36059.json"),
        include_str!("fixtures/wquw-151-frames/3879-36108.json"),
        include_str!("fixtures/wquw-151-frames/3879-36144.json"),
    ]
    .into_iter()
    .map(|raw| serde_json::from_str(raw).expect("WQUW-151 frame fixture"))
    .collect()
}

fn tsx(snapshot: &Snapshot, root_id: &str) -> String {
    generate_component(
        snapshot,
        root_id,
        &CodegenOptions {
            component_name: Some("OmissionProbe".to_owned()),
            include_diagnostics: true,
            inline_instances: true,
            ..CodegenOptions::default()
        },
    )
    .unwrap_or_else(|error| panic!("{root_id} codegen failed: {error}"))
    .tsx
}

#[test]
fn omitting_default_valued_fields_keeps_every_real_screen_byte_identical() {
    let mut checked_nodes = 0_usize;
    let mut dropped_fields = 0_usize;

    for fixture in fixtures() {
        let root_id = fixture.source.node_id.clone();
        let before = tsx(&fixture.snapshot, &root_id);

        let mut trimmed = fixture.snapshot.clone();
        for node in trimmed.nodes.values_mut() {
            dropped_fields += omit_defaults(node);
        }
        checked_nodes += trimmed.nodes.len();

        assert_eq!(
            before,
            tsx(&trimmed, &root_id),
            "omitting default-valued fields changed the TSX for screen {root_id}"
        );
    }

    // Guards against a fixture set that silently shrank to nothing.
    assert!(
        checked_nodes > 1_000,
        "expected the ten real screens to cover >1000 nodes, saw {checked_nodes}"
    );
    assert!(
        dropped_fields > 10_000,
        "expected the omission to drop >10000 fields, saw {dropped_fields}"
    );
}

/// Mirrors `SEGMENT_ONLY_KEYS` in `fast_snapshot.js`.
const SEGMENT_ONLY_KEYS: &[&str] = &[
    "start",
    "end",
    "characters",
    "fontWeight",
    "textStyleId",
    "fillStyleId",
    "listOptions",
    "indentation",
    "hyperlink",
];

#[test]
fn deduping_single_segment_text_keeps_every_real_screen_byte_identical() {
    // A lone styled text segment restates typography the TEXT node already
    // carries, and `codegen/text.rs` reads the node field first, falling back
    // to the segment only when the node lacks it.
    let mut single_segment_nodes = 0_usize;

    for fixture in fixtures() {
        let root_id = fixture.source.node_id.clone();
        let before = tsx(&fixture.snapshot, &root_id);

        let mut trimmed = fixture.snapshot.clone();
        for node in trimmed.nodes.values_mut() {
            let Some(segments) = node
                .fields
                .get_mut("styledTextSegments")
                .and_then(Value::as_array_mut)
            else {
                continue;
            };
            if segments.len() != 1 {
                continue;
            }
            single_segment_nodes += 1;
            if let Some(only) = segments[0].as_object_mut() {
                only.retain(|key, _| SEGMENT_ONLY_KEYS.contains(&key.as_str()));
            }
        }

        assert_eq!(
            before,
            tsx(&trimmed, &root_id),
            "deduping the lone text segment changed the TSX for screen {root_id}"
        );
    }

    assert!(
        single_segment_nodes > 200,
        "expected the fixtures to cover >200 single-segment text nodes, saw {single_segment_nodes}"
    );
}

#[test]
fn presence_sensitive_fields_are_never_omitted() {
    // Each of these takes a different branch when absent than when present at
    // its default, so the script must keep them verbatim.
    for field in NULL_SENSITIVE_FIELDS {
        assert!(!is_omittable(field, &Value::Null), "{field} must survive");
    }
    for (field, value) in [
        ("opacity", serde_json::json!(1)),
        ("visible", serde_json::json!(true)),
        ("layoutPositioning", serde_json::json!("AUTO")),
        ("topLeftRadius", serde_json::json!(0)),
        ("topRightRadius", serde_json::json!(0)),
        ("bottomLeftRadius", serde_json::json!(0)),
        ("bottomRightRadius", serde_json::json!(0)),
        ("strokeWeight", serde_json::json!(1)),
        ("strokeTopWeight", serde_json::json!(1)),
        ("strokeRightWeight", serde_json::json!(1)),
        ("strokeBottomWeight", serde_json::json!(1)),
        ("strokeLeftWeight", serde_json::json!(1)),
    ] {
        assert!(!is_omittable(field, &value), "{field} must survive");
    }
}

#[test]
fn every_null_field_the_fixtures_contain_is_classified_deliberately() {
    // A future manifest addition that shows up as null must be judged, not
    // silently swept into the blanket null rule.
    let mut null_fields = std::collections::BTreeSet::new();
    for fixture in fixtures() {
        for node in fixture.snapshot.nodes.values() {
            for (field, value) in &node.fields {
                if value.is_null() {
                    null_fields.insert(field.clone());
                }
            }
        }
    }
    let known = [
        "componentPropertyReferences",
        "inferredAutoLayout",
        "maxHeight",
        "maxWidth",
        "minHeight",
        "minWidth",
        "targetAspectRatio",
        "variantProperties",
    ];
    let unexpected = null_fields
        .iter()
        .filter(|field| !known.contains(&field.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        unexpected.is_empty(),
        "unclassified null-valued fields appeared: {unexpected:?}"
    );
}
