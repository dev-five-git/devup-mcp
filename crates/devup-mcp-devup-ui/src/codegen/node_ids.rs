//! Stable Figma node identity on the rendered DOM, so a runtime audit can
//! find a section or an asset by the node it was generated from instead of
//! by a guessed selector.
use std::collections::BTreeSet;

use crate::codegen::CodegenOutput;
use crate::provenance::attributes::opening_elements;

pub const NODE_ID_ATTRIBUTE: &str = "data-figma-node-id";

/// Writes `data-figma-node-id="<id>"` on the opening element generated for
/// each of `node_ids`, found through the source map, and returns the ids it
/// wrote. An element that already carries the attribute is left alone. The
/// source map's internal ranges are shifted with the text, so later checks
/// against this output still line up.
pub fn annotate_node_ids(output: &mut CodegenOutput, node_ids: &BTreeSet<String>) -> Vec<String> {
    let elements = opening_elements(&output.tsx);
    let mut targets = Vec::<(usize, String)>::new();
    for entry in &output.source_map.entries {
        let (Some(node_id), Some(ordinal)) = (&entry.node_id, entry.element_index) else {
            continue;
        };
        if !node_ids.contains(node_id) || targets.iter().any(|(_, id)| id == node_id) {
            continue;
        }
        if targets.iter().any(|(taken, _)| *taken == ordinal) {
            continue;
        }
        targets.push((ordinal, node_id.clone()));
    }
    let mut inserts = Vec::new();
    for (ordinal, node_id) in &targets {
        let Some(element) = elements.iter().find(|element| element.ordinal == *ordinal) else {
            continue;
        };
        let Some(opening) = output.tsx.get(element.start..element.end) else {
            continue;
        };
        if opening.contains(NODE_ID_ATTRIBUTE) || !opening.starts_with('<') {
            continue;
        }
        let name_end = opening[1..]
            .find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '.' | ':' | '-' | '$')))
            .map_or(opening.len(), |offset| offset + 1);
        if name_end <= 1 {
            continue;
        }
        inserts.push((
            element.start + name_end,
            format!(" {NODE_ID_ATTRIBUTE}=\"{}\"", node_id.replace('"', "")),
            node_id.clone(),
        ));
    }
    inserts.sort_by_key(|insert| std::cmp::Reverse(insert.0));
    let mut written = Vec::new();
    for (at, text, node_id) in inserts {
        output.tsx.insert_str(at, &text);
        for entry in &mut output.source_map.entries {
            if let Some(range) = &mut entry.generated_range {
                if range.start >= at {
                    range.start += text.len();
                }
                if range.end >= at {
                    range.end += text.len();
                }
            }
        }
        written.push(node_id);
    }
    written.sort();
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::{CodegenOptions, generate_component};
    use devup_mcp_figma::Snapshot;
    use serde_json::json;

    #[test]
    fn the_root_and_each_section_get_their_node_id_and_ranges_follow() {
        let snapshot: Snapshot = serde_json::from_value(json!({
            "fileKey": "ids", "roots": ["p"], "diagnostics": [], "nodes": {
                "p": {"id": "p", "type": "FRAME", "fields": {"layoutMode": "VERTICAL", "width": 400, "height": 300,
                    "childrenIds": ["a", "b"]}},
                "a": {"id": "a", "type": "FRAME", "fields": {"parentId": "p", "layoutMode": "HORIZONTAL",
                    "width": 400, "height": 100, "childrenIds": ["at"]}},
                "at": {"id": "at", "type": "TEXT", "fields": {"parentId": "a", "characters": "Hello",
                    "fontSize": 16, "fills": [{"type": "SOLID", "color": {"r": 0, "g": 0, "b": 0, "a": 1}}]}},
                "b": {"id": "b", "type": "FRAME", "fields": {"parentId": "p", "layoutMode": "HORIZONTAL",
                    "width": 400, "height": 200, "childrenIds": ["bt"]}},
                "bt": {"id": "bt", "type": "TEXT", "fields": {"parentId": "b", "characters": "World",
                    "fontSize": 16, "fills": [{"type": "SOLID", "color": {"r": 0, "g": 0, "b": 0, "a": 1}}]}}
            }
        }))
        .unwrap();
        let mut output = generate_component(&snapshot, "p", &CodegenOptions::default()).unwrap();
        let before = output.tsx.clone();
        let wanted = ["p", "a", "b"].map(String::from).into_iter().collect();
        let written = annotate_node_ids(&mut output, &wanted);
        assert_eq!(written, ["a", "b", "p"], "{before}");
        for id in ["p", "a", "b"] {
            assert!(
                output
                    .tsx
                    .contains(&format!("{NODE_ID_ATTRIBUTE}=\"{id}\"")),
                "{}",
                output.tsx
            );
        }
        let elements = opening_elements(&output.tsx);
        for entry in &output.source_map.entries {
            if let (Some(range), Some(ordinal)) = (&entry.generated_range, entry.element_index) {
                let element = elements.iter().find(|e| e.ordinal == ordinal).unwrap();
                assert!(
                    element.start <= range.start && range.end <= element.end,
                    "{entry:?}"
                );
            }
        }
        assert!(
            annotate_node_ids(&mut output, &wanted).is_empty(),
            "a second pass writes nothing"
        );
    }
}
