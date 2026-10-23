use std::collections::BTreeMap;
use serde_json::Value;

pub(super) fn request_key(asset_id: &str) -> &str {
    let id = asset_id.split(":fills:").next().unwrap_or(asset_id);
    ["node", "visible", "fill"]
        .iter()
        .find_map(|suffix| id.strip_suffix(&format!(":{suffix}")))
        .unwrap_or(id)
}

pub(super) fn request_progress(assets: &[Value]) -> (usize, usize, usize, usize) {
    let mut groups: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for asset in assets {
        groups
            .entry(request_key(asset["assetId"].as_str().unwrap_or("")))
            .or_default()
            .push(asset["status"].as_str().unwrap_or(""));
    }
    let mut counts = (groups.len(), 0, 0, 0);
    for rows in groups.values() {
        if rows.contains(&"exported") {
            counts.1 += 1;
        } else if rows.contains(&"failed") {
            counts.2 += 1;
        } else if rows.contains(&"pending") {
            counts.3 += 1;
        }
    }
    counts
}
