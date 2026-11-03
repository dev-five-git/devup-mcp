use super::*;

/// Main-axis gap comes from itemSpacing; cross-axis gap is independent.
/// Only explicitly captured flex WRAP axes participate, never grid gaps.
pub(super) fn sources(node: &devup_mcp_figma::RawNode) -> Vec<(&'static str, &'static str)> {
    let view = node.typed_view();
    if view.string("layoutWrap") != Some("WRAP") {
        return Vec::new();
    }
    let fields = match view.string("layoutMode") {
        Some("HORIZONTAL") => [
            ("columnGap", "itemSpacing"),
            ("rowGap", "counterAxisSpacing"),
        ],
        Some("VERTICAL") => [
            ("rowGap", "itemSpacing"),
            ("columnGap", "counterAxisSpacing"),
        ],
        _ => return Vec::new(),
    };
    fields
        .into_iter()
        .filter(|(_, field)| {
            !node.field_errors.contains_key(*field)
                && view
                    .number(field)
                    .is_some_and(|value| value.is_finite() && value >= 0.0)
        })
        .collect()
}

/// A similarly named property or the other axis cannot satisfy this mapping.
/// Verify the exact captured main gap, including the generator's px rounding.
pub(super) fn mapping_matches(
    snapshot: &Snapshot,
    node_id: &str,
    field: &str,
    source: &str,
) -> Option<bool> {
    if field != "itemSpacing" {
        return None;
    }
    let node = snapshot.nodes.get(node_id)?;
    if node.typed_view().string("layoutWrap") != Some("WRAP") {
        return None;
    }
    if !source.starts_with("rowGap=") && !source.starts_with("columnGap=") {
        return None;
    }
    Some(sources(node).into_iter().any(|(property, mapped)| {
        mapped == field
            && source
                .strip_prefix(&format!("{property}=\""))
                .and_then(|value| value.strip_suffix("px\""))
                .and_then(|value| value.parse::<f64>().ok())
                .zip(node.typed_view().number(field))
                .is_some_and(|(actual, captured)| {
                    actual.is_finite() && actual == (captured * 100.0).round() / 100.0
                })
    }))
}
