use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::{DevupError, ErrorCode, FigmaTarget, Snapshot};

const DEFAULT_TYPES: &[&str] = &["PAGE", "SECTION", "FRAME", "COMPONENT_SET", "COMPONENT"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchOptions {
    pub query: String,
    #[serde(default)]
    pub node_types: Vec<String>,
    #[serde(default = "default_match")]
    pub match_kind: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub node_id: String,
    pub name: String,
    pub node_type: String,
    pub page_name: Option<String>,
    pub breadcrumb: Vec<String>,
    pub canonical_url: String,
    pub match_kind: String,
    pub score: u32,
}

pub fn search_snapshot(
    snapshot: &Snapshot,
    target: &FigmaTarget,
    options: &SearchOptions,
) -> Result<Vec<SearchResult>, DevupError> {
    let query = options.query.trim();
    if query.is_empty() {
        return Err(DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "검색 query는 비어 있을 수 없습니다.",
            false,
        ));
    }
    if options.limit == 0 || options.limit > 100 {
        return Err(DevupError::new(
            ErrorCode::DevupFigmaResponseTooLarge,
            "검색 limit은 1 이상 100 이하여야 합니다.",
            false,
        ));
    }
    if !matches!(
        options.match_kind.as_str(),
        "exact" | "normalized" | "fuzzy"
    ) {
        return Err(DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "match는 exact, normalized 또는 fuzzy여야 합니다.",
            false,
        ));
    }
    let types = if options.node_types.is_empty() {
        DEFAULT_TYPES
            .iter()
            .map(|value| (*value).to_owned())
            .collect()
    } else {
        options
            .node_types
            .iter()
            .map(|value| value.to_ascii_uppercase())
            .collect::<Vec<_>>()
    };
    let parents = parent_index(snapshot);
    let normalized_query = normalize(query);
    let mut results = snapshot
        .nodes
        .values()
        .filter(|node| types.iter().any(|node_type| node_type == &node.node_type))
        .filter_map(|node| {
            let name = node.typed_view().name()?;
            let (kind, score) =
                match_score(name, query, &normalized_query, options.match_kind.as_str())?;
            let breadcrumb = breadcrumb(snapshot, &parents, &node.id);
            let page_name = ancestor_page(snapshot, &parents, &node.id);
            Some(SearchResult {
                node_id: node.id.clone(),
                name: name.to_owned(),
                node_type: node.node_type.clone(),
                page_name,
                breadcrumb,
                canonical_url: canonical_url(target, &node.id),
                match_kind: kind.to_owned(),
                score,
            })
        })
        .collect::<Vec<_>>();
    results.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.node_id.cmp(&right.node_id))
    });
    results.truncate(options.limit);
    Ok(results)
}

fn match_score<'a>(
    name: &str,
    query: &str,
    normalized_query: &str,
    mode: &str,
) -> Option<(&'a str, u32)> {
    if name == query {
        return Some(("exact", 400));
    }
    if mode == "exact" {
        return None;
    }
    let normalized_name = normalize(name);
    if normalized_name == normalized_query {
        return Some(("normalized-exact", 300));
    }
    if normalized_name.starts_with(normalized_query) {
        return Some(("prefix", 200));
    }
    if normalized_name.contains(normalized_query) {
        return Some(("contains", 100));
    }
    if mode != "fuzzy" {
        return None;
    }
    let distance = levenshtein(&normalized_name, normalized_query);
    let threshold = normalized_query.chars().count().div_ceil(4).clamp(1, 4);
    (distance <= threshold).then_some(("fuzzy", 50_u32.saturating_sub(distance as u32)))
}

fn normalize(value: &str) -> String {
    value
        .nfc()
        .flat_map(char::to_lowercase)
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn parent_index(snapshot: &Snapshot) -> BTreeMap<String, String> {
    let mut parents = BTreeMap::new();
    for node in snapshot.nodes.values() {
        if let Some(parent) = node.typed_view().string("parentId") {
            parents.insert(node.id.clone(), parent.to_owned());
        }
        for child in node.typed_view().child_ids() {
            parents
                .entry(child.to_owned())
                .or_insert_with(|| node.id.clone());
        }
    }
    parents
}

fn breadcrumb(
    snapshot: &Snapshot,
    parents: &BTreeMap<String, String>,
    node_id: &str,
) -> Vec<String> {
    let mut ids = Vec::new();
    let mut current = Some(node_id);
    while let Some(id) = current {
        ids.push(id.to_owned());
        current = parents.get(id).map(String::as_str);
        if ids.len() > snapshot.nodes.len() {
            break;
        }
    }
    ids.reverse();
    ids.into_iter()
        .filter_map(|id| {
            snapshot
                .nodes
                .get(&id)?
                .typed_view()
                .name()
                .map(str::to_owned)
        })
        .collect()
}

fn ancestor_page(
    snapshot: &Snapshot,
    parents: &BTreeMap<String, String>,
    node_id: &str,
) -> Option<String> {
    let mut current = Some(node_id);
    while let Some(id) = current {
        let node = snapshot.nodes.get(id)?;
        if node.node_type == "PAGE" {
            return node.typed_view().name().map(str::to_owned);
        }
        current = parents.get(id).map(String::as_str);
    }
    None
}

fn canonical_url(target: &FigmaTarget, node_id: &str) -> String {
    let node_id = node_id.replace(':', "-");
    if let Some(branch_key) = &target.branch_key {
        format!(
            "https://www.figma.com/branch/{}/{branch_key}/devup?node-id={node_id}",
            target.file_key
        )
    } else {
        format!(
            "https://www.figma.com/design/{}/devup?node-id={node_id}",
            target.file_key
        )
    }
}

fn levenshtein(left: &str, right: &str) -> usize {
    let right = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    for (left_index, left_char) in left.chars().enumerate() {
        let mut current = vec![left_index + 1];
        for (right_index, right_char) in right.iter().enumerate() {
            current.push(
                (current[right_index] + 1)
                    .min(previous[right_index + 1] + 1)
                    .min(previous[right_index] + usize::from(left_char != *right_char)),
            );
        }
        previous = current;
    }
    previous[right.len()]
}

fn default_match() -> String {
    "normalized".to_owned()
}

fn default_limit() -> usize {
    20
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::RawNode;

    #[test]
    fn ranks_exact_normalized_prefix_and_contains_with_breadcrumbs() {
        let nodes = [
            ("0:1", "PAGE", "Screens", vec!["1:1"]),
            ("1:1", "SECTION", "A : STORY-F-PROOFREAD", vec!["2:1"]),
            ("2:1", "FRAME", "Story F Proofread Detail", vec![]),
        ]
        .into_iter()
        .map(|(id, node_type, name, children)| {
            (
                id.to_owned(),
                RawNode {
                    id: id.to_owned(),
                    node_type: node_type.to_owned(),
                    fields: serde_json::from_value(json!({
                        "name": name,
                        "childrenIds": children,
                    }))
                    .unwrap(),
                    extra: Default::default(),
                    field_errors: Default::default(),
                },
            )
        })
        .collect();
        let snapshot = Snapshot {
            file_key: "85CgSws3o5XsLv7aAwWJyS".to_owned(),
            version: None,
            roots: vec!["0:1".to_owned()],
            nodes,
            diagnostics: Vec::new(),
        };
        let target = FigmaTarget {
            file_key: snapshot.file_key.clone(),
            node_id: None,
            branch_key: None,
        };
        let results = search_snapshot(
            &snapshot,
            &target,
            &SearchOptions {
                query: "a:story-f-proofread".to_owned(),
                node_types: Vec::new(),
                match_kind: "normalized".to_owned(),
                limit: 20,
            },
        )
        .unwrap();
        assert_eq!(results[0].node_id, "1:1");
        assert_eq!(results[0].match_kind, "normalized-exact");
        assert_eq!(results[0].page_name.as_deref(), Some("Screens"));
        assert_eq!(results[0].breadcrumb, ["Screens", "A : STORY-F-PROOFREAD"]);
        assert!(results[0].canonical_url.ends_with("node-id=1-1"));
    }

    #[test]
    fn fuzzy_matching_is_opt_in() {
        assert!(match_score("Proofread", "Proofred", "proofred", "normalized").is_none());
        assert_eq!(
            match_score("Proofread", "Proofred", "proofred", "fuzzy"),
            Some(("fuzzy", 49))
        );
    }
}
