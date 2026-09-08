//! A field the code reads must be a field the collector asks Figma for.
//!
//! Twice now a rule has been written against a node field that was never
//! collected, so it read nothing and silently took the wrong branch: text
//! truncation defaulted to on because an absent value is not `DISABLED`, and a
//! component set could not find its default variant by name. Both looked
//! correct in the pinned corpus, whose captures carry the fields, and were only
//! wrong against a live file — which is exactly the gap a fixture cannot show.

use std::{collections::BTreeSet, fs, path::Path};

/// Names that are read through the same accessors but never come from a Figma
/// node, so the manifest has nothing to say about them.
const NOT_NODE_FIELDS: &[&str] = &[
    // Written by our own scripts onto the node record.
    "parentId",
    "parentType",
    "childrenIds",
    "styledTextSegments",
    // Envelope, pagination and probe records, not nodes.
    "breadcrumb",
    "childCount",
    "complete",
    "devupTokens",
    "directChildCount",
    "estimatedSerializedBytes",
    "pageChildIndex",
    "projectionTruncated",
    "subtreeNodeCount",
    "textPreview",
    // Read only on the explore path, whose script reads the node directly
    // rather than through the manifest.
    "absoluteBoundingBox",
    "annotations",
];

fn read_sources(directory: &Path, into: &mut String) {
    for entry in fs::read_dir(directory).expect("source directory") {
        let path = entry.expect("source entry").path();
        if path.is_dir() {
            read_sources(&path, into);
        } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
            into.push_str(&fs::read_to_string(&path).expect("source file"));
            into.push('\n');
        }
    }
}

#[test]
fn every_field_the_code_reads_is_a_field_the_collector_requests() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut source = String::new();
    read_sources(&crates.join("devup-mcp-figma/src"), &mut source);
    read_sources(&crates.join("devup-mcp-devup-ui/src"), &mut source);

    let manifest: BTreeSet<String> = serde_json::from_str(
        &fs::read_to_string(crates.join("devup-mcp-figma/src/plugin_api_manifest.json"))
            .expect("manifest"),
    )
    .expect("manifest is a list of field names");

    // `view.string("x")` and friends are how a node field is read.
    let mut missing = BTreeSet::new();
    for accessor in [".value(\"", ".string(\"", ".number(\"", ".bool(\""] {
        let mut rest = source.as_str();
        while let Some(at) = rest.find(accessor) {
            rest = &rest[at + accessor.len()..];
            let Some(end) = rest.find('"') else { break };
            let field = &rest[..end];
            if !field.is_empty()
                && field.chars().all(|c| c.is_ascii_alphanumeric())
                && !manifest.contains(field)
                && !NOT_NODE_FIELDS.contains(&field)
            {
                missing.insert(field.to_owned());
            }
        }
    }

    assert!(
        missing.is_empty(),
        "these node fields are read but never collected, so they are always \
         absent against a live file: {missing:?}. Add them to \
         plugin_api_manifest.json, or list them in NOT_NODE_FIELDS with the \
         reason they are not node fields."
    );
}
