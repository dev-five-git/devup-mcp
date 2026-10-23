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
    /// Set when the page or an ancestor name marks the node as trash,
    /// archive, draft or copy: the keyword that matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excluded: Option<String>,
}

const EXCLUDED_KEYWORDS: &[&str] = &[
    "trash",
    "archive",
    "draft",
    "copy",
    "휴지통",
    "보관",
    "초안",
    "복사",
    "pdf",
    "저장용",
    "export",
    "출력",
];

fn excluded_reason(breadcrumb: &[String], page_name: Option<&str>) -> Option<String> {
    page_name
        .into_iter()
        .chain(breadcrumb.iter().map(String::as_str))
        .find_map(|name| {
            let lowered = name.to_lowercase();
            EXCLUDED_KEYWORDS
                .iter()
                .find(|keyword| lowered.contains(*keyword))
                .map(|keyword| format!("{keyword} ({name})"))
        })
}

pub fn search_snapshot(
    snapshot: &Snapshot,
    target: &FigmaTarget,
    options: &SearchOptions,
) -> Result<Vec<SearchResult>, DevupError> {
    let query = options.query.trim();
    let list_by_type = query.is_empty() || query == "*";
    if list_by_type && options.node_types.is_empty() {
        return Err(DevupError::new(
            ErrorCode::DevupInvalidInput,
            "Search query cannot be empty unless nodeTypes is given. Pass nodeTypes (for example [\"SECTION\"]) with an empty query or \"*\" to list every node of those types.",
            false,
        ));
    }
    if options.limit == 0 || options.limit > 100 {
        return Err(DevupError::new(
            ErrorCode::DevupInvalidInput,
            "Search limit must be between 1 and 100.",
            false,
        ));
    }
    if !matches!(
        options.match_kind.as_str(),
        "exact" | "normalized" | "fuzzy"
    ) {
        return Err(DevupError::new(
            ErrorCode::DevupInvalidInput,
            "match must be exact, normalized, or fuzzy.",
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
            let (kind, score) = if list_by_type {
                ("type", 100)
            } else {
                match_score(name, query, &normalized_query, options.match_kind.as_str())?
            };
            let breadcrumb = breadcrumb(snapshot, &parents, &node.id);
            let page_name = ancestor_page(snapshot, &parents, &node.id);
            let excluded = excluded_reason(&breadcrumb, page_name.as_deref());
            Some(SearchResult {
                excluded,
                node_id: node.id.clone(),
                name: name.to_owned(),
                node_type: node.node_type.clone(),
                page_name,
                breadcrumb,
                canonical_url: target.link(Some(&node.id)),
                match_kind: kind.to_owned(),
                score,
            })
        })
        .collect::<Vec<_>>();
    results.sort_by(|left, right| {
        left.excluded
            .is_some()
            .cmp(&right.excluded.is_some())
            .then_with(|| right.score.cmp(&left.score))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.node_id.cmp(&right.node_id))
    });
    results.truncate(options.limit);
    Ok(results)
}

/// One warning per name that appears both on an excluded page (trash, archive,
/// PDF copy...) and on a normal page, so callers do not pick the discarded copy.
pub fn excluded_duplicate_warnings(results: &[SearchResult]) -> Vec<String> {
    let mut warnings = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for excluded in results.iter().filter(|result| result.excluded.is_some()) {
        let key = normalize(&excluded.name);
        let normal = results
            .iter()
            .find(|other| other.excluded.is_none() && normalize(&other.name) == key);
        if let Some(normal) = normal
            && seen.insert(key)
        {
            warnings.push(format!(
                "\"{}\" exists on a normal page ({}, node {}) and on an excluded page ({}, node {}). Prefer the normal one; the excluded copy is trash, archive, draft or an export copy.",
                excluded.name,
                normal.page_name.as_deref().unwrap_or("?"),
                normal.node_id,
                excluded.excluded.as_deref().unwrap_or("excluded"),
                excluded.node_id,
            ));
        }
    }
    warnings
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

    fn typed_snapshot() -> (Snapshot, FigmaTarget) {
        let nodes = [
            ("0:1", "PAGE", "Screens", vec!["1:1", "1:2"]),
            ("1:1", "SECTION", "About", vec![]),
            ("1:2", "FRAME", "Home", vec![]),
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
            file_key: "key".to_owned(),
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
        (snapshot, target)
    }

    fn options(query: &str, node_types: &[&str]) -> SearchOptions {
        SearchOptions {
            query: query.to_owned(),
            node_types: node_types.iter().map(|t| (*t).to_owned()).collect(),
            match_kind: "normalized".to_owned(),
            limit: 20,
        }
    }

    #[test]
    fn an_empty_or_star_query_with_node_types_lists_every_node_of_that_type() {
        let (snapshot, target) = typed_snapshot();
        for query in ["", " ", "*"] {
            let results =
                search_snapshot(&snapshot, &target, &options(query, &["section"])).unwrap();
            assert_eq!(results.len(), 1, "query {query:?}");
            assert_eq!(results[0].node_id, "1:1");
            assert_eq!(results[0].match_kind, "type");
        }
    }

    #[test]
    fn matches_under_a_trash_page_are_flagged_and_sorted_last() {
        let nodes = [
            ("0:1", "PAGE", "휴지통", vec!["1:1"]),
            ("0:2", "PAGE", "디자인 시스템", vec!["1:2"]),
            ("1:1", "FRAME", "특허", vec![]),
            ("1:2", "FRAME", "특허", vec![]),
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
            file_key: "key".to_owned(),
            version: None,
            roots: vec!["0:1".to_owned(), "0:2".to_owned()],
            nodes,
            diagnostics: Vec::new(),
        };
        let target = FigmaTarget {
            file_key: "key".to_owned(),
            node_id: None,
            branch_key: None,
        };
        let results = search_snapshot(&snapshot, &target, &options("특허", &[])).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].node_id, "1:2");
        assert!(results[0].excluded.is_none());
        assert_eq!(results[1].node_id, "1:1");
        assert!(results[1].excluded.as_deref().unwrap().contains("휴지통"));
    }

    #[test]
    fn a_name_on_both_an_excluded_and_a_normal_page_gets_one_warning() {
        let make = |id: &str, page: &str, excluded: Option<&str>| SearchResult {
            node_id: id.to_owned(),
            name: "특허".to_owned(),
            node_type: "FRAME".to_owned(),
            page_name: Some(page.to_owned()),
            breadcrumb: Vec::new(),
            canonical_url: String::new(),
            match_kind: "exact".to_owned(),
            score: 400,
            excluded: excluded.map(str::to_owned),
        };
        let results = [
            make("1:2", "디자인 시스템", None),
            make("1:1", "휴지통", Some("휴지통 (휴지통)")),
            make("1:3", "PDF", Some("pdf (PDF)")),
        ];
        let warnings = excluded_duplicate_warnings(&results);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("1:2") && warnings[0].contains("1:1"));
        assert!(excluded_duplicate_warnings(&results[..1]).is_empty());
        assert!(excluded_duplicate_warnings(&results[1..]).is_empty());
    }

    #[test]
    fn pdf_and_export_pages_are_flagged_as_excluded() {
        for page in ["PDF", "저장용", "Export", "출력본"] {
            assert!(excluded_reason(&[], Some(page)).is_some(), "{page}");
        }
        assert!(excluded_reason(&[], Some("디자인 시스템")).is_none());
    }

    #[test]
    fn an_empty_query_without_node_types_is_invalid_input() {
        let (snapshot, target) = typed_snapshot();
        let error = search_snapshot(&snapshot, &target, &options("", &[])).unwrap_err();
        assert_eq!(error.code, ErrorCode::DevupInvalidInput);
    }

    #[test]
    fn a_bad_limit_or_match_kind_is_invalid_input() {
        let (snapshot, target) = typed_snapshot();
        for limit in [0, 101] {
            let mut bad = options("a", &[]);
            bad.limit = limit;
            let error = search_snapshot(&snapshot, &target, &bad).unwrap_err();
            assert_eq!(error.code, ErrorCode::DevupInvalidInput, "limit {limit}");
        }
        let mut bad = options("a", &[]);
        bad.match_kind = "regex".to_owned();
        let error = search_snapshot(&snapshot, &target, &bad).unwrap_err();
        assert_eq!(error.code, ErrorCode::DevupInvalidInput);
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
