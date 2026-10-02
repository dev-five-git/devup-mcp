//! Field selection for the diagnostic outputs.
//!
//! A raw snapshot of one screen can run to tens of thousands of tokens, and the
//! usual question ("what are the exact fills / spacing / type values here?")
//! needs a handful of properties. `fields` narrows `rawSnapshot`, `rawPayload`
//! and the sourceMap entries to the named raw Figma properties, so the answer
//! fits in one response instead of being cut off and re-queried.
use std::collections::BTreeSet;

use serde_json::Value;

/// Group names a caller can use instead of listing raw property names.
const GROUPS: &[(&str, &[&str])] = &[
    (
        "colors",
        &[
            "fills",
            "strokes",
            "effects",
            "opacity",
            "boundVariables",
            "fillStyleId",
            "strokeStyleId",
        ],
    ),
    (
        "spacing",
        &[
            "itemSpacing",
            "counterAxisSpacing",
            "paddingTop",
            "paddingRight",
            "paddingBottom",
            "paddingLeft",
            "layoutMode",
            "primaryAxisAlignItems",
            "counterAxisAlignItems",
        ],
    ),
    (
        "typography",
        &[
            "fontSize",
            "fontName",
            "fontWeight",
            "lineHeight",
            "letterSpacing",
            "textStyleId",
            "styledTextSegments",
            "textAlignHorizontal",
            "characters",
        ],
    ),
    ("shadow", &["effects"]),
    (
        "radius",
        &[
            "cornerRadius",
            "topLeftRadius",
            "topRightRadius",
            "bottomLeftRadius",
            "bottomRightRadius",
        ],
    ),
    (
        "size",
        &[
            "width",
            "height",
            "layoutSizingHorizontal",
            "layoutSizingVertical",
            "layoutGrow",
            "absoluteBoundingBox",
        ],
    ),
];

/// Properties every pruned node keeps so the tree stays navigable.
const STRUCTURE: &[&str] = &["childrenIds", "parentId", "name", "visible", "isAsset"];

/// Expand group names and drop blanks. Empty input means "no selection".
pub(super) fn expand(requested: &[String]) -> BTreeSet<String> {
    let mut selected = BTreeSet::new();
    for name in requested.iter().map(|n| n.trim()).filter(|n| !n.is_empty()) {
        match GROUPS
            .iter()
            .find(|(group, _)| group.eq_ignore_ascii_case(name))
        {
            Some((_, members)) => selected.extend(members.iter().map(|m| (*m).to_owned())),
            None => {
                selected.insert(name.to_owned());
            }
        }
    }
    selected
}

/// Keep only the selected properties on every node of a raw snapshot or
/// payload. Node identity, type and tree links are always kept.
pub(super) fn prune_nodes(raw: &mut Value, selected: &BTreeSet<String>) {
    if selected.is_empty() {
        return;
    }
    match raw {
        Value::Object(object) => {
            if let Some(Value::Object(nodes)) = object.get_mut("nodes") {
                for node in nodes.values_mut() {
                    if let Some(Value::Object(fields)) = node.get_mut("fields") {
                        fields.retain(|key, _| {
                            selected.contains(key) || STRUCTURE.contains(&key.as_str())
                        });
                    }
                    if let Some(extra) = node.get_mut("extra").and_then(Value::as_object_mut) {
                        extra.clear();
                    }
                }
            }
            for value in object.values_mut() {
                prune_nodes(value, selected);
            }
        }
        Value::Array(values) => values.iter_mut().for_each(|v| prune_nodes(v, selected)),
        _ => {}
    }
}

/// Keep sourceMap entries whose raw `property` was selected.
pub(super) fn filter_entries(entries: &mut Vec<Value>, selected: &BTreeSet<String>) {
    if !selected.is_empty() {
        entries.retain(|e| e["property"].as_str().is_some_and(|p| selected.contains(p)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn groups_expand_and_names_pass_through() {
        let selected = expand(&["Typography".into(), "cornerRadius".into(), " ".into()]);
        assert!(selected.contains("lineHeight") && selected.contains("cornerRadius"));
        assert!(!selected.contains("fills"));
    }

    #[test]
    fn pruning_keeps_structure_and_selected_fields_only() {
        let mut raw = json!({"snapshot":{"nodes":{"1:1":{"id":"1:1","type":"TEXT",
            "fields":{"fontSize":16,"fills":[1],"childrenIds":[],"name":"t"},"extra":{"big":1}}}}});
        prune_nodes(&mut raw, &expand(&["fontSize".into()]));
        let node = &raw["snapshot"]["nodes"]["1:1"];
        assert_eq!(node["id"], "1:1");
        assert!(node["fields"].get("fills").is_none());
        assert_eq!(node["fields"]["fontSize"], 16);
        assert!(node["fields"].get("childrenIds").is_some());
        assert!(node["extra"].as_object().unwrap().is_empty());
    }

    #[test]
    fn empty_selection_changes_nothing() {
        let mut raw = json!({"nodes":{"a":{"fields":{"fills":[1]}}}});
        let before = raw.clone();
        prune_nodes(&mut raw, &expand(&[]));
        assert_eq!(raw, before);
        let mut entries = vec![json!({"property":"w"})];
        filter_entries(&mut entries, &expand(&[]));
        assert_eq!(entries.len(), 1);
    }
}
