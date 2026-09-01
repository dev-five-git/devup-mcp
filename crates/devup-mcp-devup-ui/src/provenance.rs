use std::collections::{BTreeMap, BTreeSet};

use devup_mcp_figma::{DevupError, ErrorCode, FidelityImpact, Snapshot, discover_asset_manifest};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::codegen::CodegenOutput;

const START: &str = "\u{e000}DEVUP_PROVENANCE_START:";
const END: &str = "\u{e000}DEVUP_PROVENANCE_END:";
const CLOSE: char = '\u{e001}';

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvenanceEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_range: Option<GeneratedRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_pointer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub property: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_id: Option<String>,
    pub resolution: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceMap {
    pub version: u32,
    pub entries: Vec<ProvenanceEntry>,
}

impl SourceMap {
    pub fn empty() -> Self {
        Self {
            version: 1,
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectionDisposition {
    Emitted,
    Flattened,
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionTraceEntry {
    pub node_id: String,
    pub disposition: ProjectionDisposition,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_range: Option<GeneratedRange>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionTrace {
    pub root_node_id: String,
    pub entries: Vec<ProjectionTraceEntry>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FidelityCoverage {
    pub total: usize,
    pub covered: usize,
    pub basis_points: u16,
}

impl FidelityCoverage {
    fn new(total: usize, covered: usize) -> Self {
        let basis_points = if total == 0 {
            10_000
        } else {
            ((covered.min(total) as u128 * 10_000) / total as u128) as u16
        };
        Self {
            total,
            covered: covered.min(total),
            basis_points,
        }
    }

    pub fn complete(self) -> bool {
        self.total == self.covered
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FidelityImpactCounts {
    pub none: usize,
    pub approximated: usize,
    pub lossy: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FidelityReport {
    pub syntax_valid: bool,
    pub nodes: FidelityCoverage,
    pub text: FidelityCoverage,
    pub variables: FidelityCoverage,
    pub typography: FidelityCoverage,
    pub assets: FidelityCoverage,
    pub layout: FidelityCoverage,
    pub impacts: FidelityImpactCounts,
}

impl FidelityReport {
    pub fn strict_compatible(&self) -> bool {
        self.syntax_valid
            && self.nodes.complete()
            && self.text.complete()
            && self.variables.complete()
            && self.typography.complete()
            && self.assets.complete()
            && self.layout.complete()
            && self.impacts.lossy == 0
            && self.impacts.failed == 0
    }
}

pub(crate) fn build_projection_trace(
    snapshot: &Snapshot,
    root_id: &str,
    tsx: &str,
    source_map: &SourceMap,
) -> ProjectionTrace {
    let active = active_nodes(snapshot, root_id);
    let emitted = source_map
        .entries
        .iter()
        .filter(|entry| entry.property.is_none() && entry.resolution == "node")
        .filter_map(|entry| Some((entry.node_id.clone()?, entry.generated_range.clone()?)))
        .collect::<BTreeMap<_, _>>();
    let asset_nodes = discover_asset_manifest(snapshot)
        .assets
        .into_iter()
        .map(|asset| asset.node_id)
        .collect::<BTreeSet<_>>();
    let mut entries = Vec::new();
    let mut represented_by_parent = BTreeSet::new();
    let mut ignored_by_parent = BTreeSet::new();
    for (node_id, parent_id) in active {
        let node = snapshot.nodes.get(&node_id);
        let range = emitted.get(&node_id);
        let projection =
            if node_id == root_id && node.is_some_and(|node| node.node_type == "SECTION") {
                Some((ProjectionDisposition::Flattened, "section-root", None))
            } else if let Some(range) = range.filter(|range| range.start < range.end) {
                Some((
                    ProjectionDisposition::Emitted,
                    "direct",
                    Some((*range).clone()),
                ))
            } else if range.is_some()
                || node.is_some_and(|node| node.typed_view().bool("visible") == Some(false))
            {
                Some((ProjectionDisposition::Ignored, "hidden", None))
            } else if parent_id
                .as_ref()
                .is_some_and(|parent_id| ignored_by_parent.contains(parent_id))
            {
                Some((ProjectionDisposition::Ignored, "hidden-ancestor", None))
            } else if parent_id.as_ref().is_some_and(|parent_id| {
                asset_nodes.contains(parent_id)
                    || represented_by_parent.contains(parent_id)
                    || snapshot.nodes.get(parent_id).is_some_and(|parent| {
                        parent.node_type == "INSTANCE"
                            || parent.typed_view().bool("isAsset") == Some(true)
                    })
                    || emitted.get(parent_id).is_some_and(|range| {
                        range.end <= tsx.len()
                            && (tsx[range.start..range.end].contains("maskImage=")
                                || tsx[range.start..range.end].contains("<Image"))
                    })
            }) {
                Some((
                    ProjectionDisposition::Flattened,
                    "represented-by-parent",
                    None,
                ))
            } else {
                None
            };
        if let Some((disposition, reason, generated_range)) = projection {
            if disposition == ProjectionDisposition::Flattened && reason == "represented-by-parent"
            {
                represented_by_parent.insert(node_id.clone());
            }
            if disposition == ProjectionDisposition::Ignored {
                ignored_by_parent.insert(node_id.clone());
            }
            entries.push(ProjectionTraceEntry {
                node_id,
                disposition,
                reason: reason.to_owned(),
                generated_range,
            });
        }
    }
    ProjectionTrace {
        root_node_id: root_id.to_owned(),
        entries,
    }
}

pub fn validate_fidelity(
    snapshot: &Snapshot,
    root_id: &str,
    output: &CodegenOutput,
) -> Result<FidelityReport, DevupError> {
    let expected = active_nodes(snapshot, root_id)
        .into_iter()
        .map(|(node_id, _)| node_id)
        .collect::<BTreeSet<_>>();
    let mut observed = BTreeSet::new();
    let mut duplicates = Vec::new();
    let mut invalid_ranges = Vec::new();
    for entry in &output.projection_trace.entries {
        if !observed.insert(entry.node_id.clone()) {
            duplicates.push(entry.node_id.clone());
        }
        if let Some(range) = &entry.generated_range
            && (range.start >= range.end
                || range.end > output.tsx.len()
                || !output.tsx.is_char_boundary(range.start)
                || !output.tsx.is_char_boundary(range.end))
        {
            invalid_ranges.push(entry.node_id.clone());
        }
    }
    let missing = expected.difference(&observed).cloned().collect::<Vec<_>>();
    let unexpected = observed.difference(&expected).cloned().collect::<Vec<_>>();
    if !duplicates.is_empty()
        || !missing.is_empty()
        || !unexpected.is_empty()
        || !invalid_ranges.is_empty()
    {
        return Err(DevupError::with_details(
            ErrorCode::DevupCodegenFailed,
            "projection trace가 source node를 정확히 한 번씩 설명하지 못했습니다.",
            false,
            json!({
                "missingNodeIds": missing,
                "duplicateNodeIds": duplicates,
                "unexpectedNodeIds": unexpected,
                "invalidRangeNodeIds": invalid_ranges
            }),
        ));
    }

    let active_non_ignored = output
        .projection_trace
        .entries
        .iter()
        .filter(|entry| entry.disposition != ProjectionDisposition::Ignored)
        .map(|entry| entry.node_id.as_str())
        .collect::<BTreeSet<_>>();
    let text_nodes = active_non_ignored
        .iter()
        .filter(|node_id| {
            snapshot
                .nodes
                .get(**node_id)
                .is_some_and(|node| node.node_type == "TEXT")
        })
        .copied()
        .collect::<BTreeSet<_>>();
    let covered_text = text_nodes
        .iter()
        .filter(|node_id| {
            snapshot
                .nodes
                .get(**node_id)
                .and_then(|node| node.typed_view().string("characters"))
                .is_none_or(str::is_empty)
                || output.source_map.entries.iter().any(|entry| {
                    entry.node_id.as_deref() == Some(**node_id)
                        && entry.property.as_deref() == Some("characters")
                })
        })
        .count();
    let variables = output
        .source_map
        .entries
        .iter()
        .filter_map(|entry| {
            entry
                .variable_id
                .as_deref()
                .map(|id| (entry.node_id.as_deref(), id))
        })
        .collect::<BTreeSet<_>>();
    let typography = typography_sources(snapshot, &text_nodes);
    let covered_typography = typography
        .iter()
        .filter(|(node_id, style_id)| {
            output.source_map.entries.iter().any(|entry| {
                entry.node_id.as_deref() == Some(*node_id)
                    && entry.style_id.as_deref() == Some(*style_id)
            })
        })
        .count();
    let assets = discover_asset_manifest(snapshot)
        .assets
        .into_iter()
        .filter(|asset| active_non_ignored.contains(asset.node_id.as_str()))
        .map(|asset| asset.node_id)
        .collect::<BTreeSet<_>>();
    let covered_assets = assets
        .iter()
        .filter(|node_id| observed.contains(*node_id))
        .count();
    let layout_nodes = active_non_ignored
        .iter()
        .filter(|node_id| {
            snapshot.nodes.get(**node_id).is_some_and(|node| {
                LAYOUT_FIELDS
                    .iter()
                    .any(|field| node.typed_view().value(field).is_some())
            })
        })
        .copied()
        .collect::<BTreeSet<_>>();
    let covered_layout = layout_nodes
        .iter()
        .filter(|node_id| observed.contains(**node_id))
        .count();
    let mut impacts = FidelityImpactCounts::default();
    for diagnostic in &output.diagnostics {
        match diagnostic.fidelity_impact() {
            FidelityImpact::None => impacts.none += 1,
            FidelityImpact::Approximated => impacts.approximated += 1,
            FidelityImpact::Lossy => impacts.lossy += 1,
            FidelityImpact::Failed => impacts.failed += 1,
        }
    }
    Ok(FidelityReport {
        syntax_valid: true,
        nodes: FidelityCoverage::new(expected.len(), observed.len()),
        text: FidelityCoverage::new(text_nodes.len(), covered_text),
        variables: FidelityCoverage::new(variables.len(), variables.len()),
        typography: FidelityCoverage::new(typography.len(), covered_typography),
        assets: FidelityCoverage::new(assets.len(), covered_assets),
        layout: FidelityCoverage::new(layout_nodes.len(), covered_layout),
        impacts,
    })
}

fn active_nodes(snapshot: &Snapshot, root_id: &str) -> Vec<(String, Option<String>)> {
    let mut output = Vec::new();
    let Some(root) = snapshot.nodes.get(root_id) else {
        return output;
    };
    output.push((root_id.to_owned(), None));
    let start = if root.node_type == "SECTION" {
        root.typed_view().child_ids().next()
    } else {
        Some(root_id)
    };
    let Some(start) = start else {
        return output;
    };
    let mut stack = if start == root_id {
        let mut children = root
            .typed_view()
            .child_ids()
            .map(|child| (child.to_owned(), Some(root_id.to_owned())))
            .collect::<Vec<_>>();
        children.reverse();
        children
    } else {
        vec![(start.to_owned(), Some(root_id.to_owned()))]
    };
    let mut seen = BTreeSet::from([root_id.to_owned()]);
    while let Some((node_id, parent_id)) = stack.pop() {
        if !seen.insert(node_id.clone()) {
            continue;
        }
        let Some(node) = snapshot.nodes.get(&node_id) else {
            continue;
        };
        output.push((node_id.clone(), parent_id));
        let mut children = node.typed_view().child_ids().collect::<Vec<_>>();
        children.reverse();
        for child_id in children {
            stack.push((child_id.to_owned(), Some(node_id.clone())));
        }
    }
    output
}

fn typography_sources<'a>(
    snapshot: &'a Snapshot,
    text_nodes: &BTreeSet<&'a str>,
) -> BTreeSet<(&'a str, &'a str)> {
    let mut styles = BTreeSet::new();
    for node_id in text_nodes {
        let Some(node) = snapshot.nodes.get(*node_id) else {
            continue;
        };
        if let Some(style_id) = node
            .typed_view()
            .string("textStyleId")
            .filter(|id| !id.is_empty())
        {
            styles.insert((*node_id, style_id));
        }
        if let Some(segments) = node
            .typed_view()
            .value("styledTextSegments")
            .and_then(serde_json::Value::as_array)
        {
            for style_id in segments.iter().filter_map(|segment| {
                segment
                    .get("textStyleId")
                    .and_then(serde_json::Value::as_str)
                    .filter(|id| !id.is_empty())
            }) {
                styles.insert((*node_id, style_id));
            }
        }
    }
    styles
}

const LAYOUT_FIELDS: &[&str] = &[
    "layoutMode",
    "layoutPositioning",
    "width",
    "height",
    "itemSpacing",
    "paddingTop",
    "paddingRight",
    "paddingBottom",
    "paddingLeft",
];

pub(crate) fn mark_node(node_id: &str, rendered: String) -> String {
    format!("{START}{node_id}{CLOSE}{rendered}{END}{node_id}{CLOSE}")
}

pub(crate) fn finalize_tsx(
    marked: &str,
    snapshot: &Snapshot,
    variable_tokens: &BTreeMap<String, String>,
    style_tokens: &BTreeMap<String, String>,
) -> (String, SourceMap) {
    let (tsx, node_ranges) = strip_markers(marked);
    let mut entries = Vec::new();
    for (node_id, range) in node_ranges {
        let Some(node) = snapshot.nodes.get(&node_id) else {
            continue;
        };
        entries.push(ProvenanceEntry {
            generated_range: Some(range.clone()),
            json_pointer: None,
            node_id: Some(node_id.clone()),
            property: None,
            variable_id: None,
            style_id: None,
            resolution: "node".to_owned(),
        });
        let node_source = &tsx[range.start..range.end];
        let Some(open_relative) = node_source.find('<') else {
            continue;
        };
        let Some(close_relative) = node_source[open_relative..].find('>') else {
            continue;
        };
        let opening_end = open_relative + close_relative + 1;
        let opening = &node_source[open_relative..opening_end];
        let component_start = open_relative + 1;
        let component_end = opening[1..]
            .find(|character: char| character.is_whitespace() || matches!(character, '>' | '/'))
            .map_or(opening.len(), |offset| offset + 1);
        if component_end > component_start {
            entries.push(generated_entry(
                range.start + component_start,
                range.start + component_end,
                &node_id,
                "type",
                None,
                None,
                "exact",
            ));
        }

        for (prop, property) in PROP_SOURCES {
            let needle = format!("{prop}=\"");
            let Some(start) = find_prop(opening, &needle) else {
                continue;
            };
            let value_start = start + needle.len();
            let Some(value_end) = opening[value_start..].find('"') else {
                continue;
            };
            let end = value_start + value_end + 1;
            let value = &opening[value_start..value_start + value_end];
            let variable_id = find_resource_id(value, variable_tokens, '$');
            let style_id = (*prop == "typography")
                .then(|| {
                    style_tokens
                        .iter()
                        .find_map(|(id, token)| (token == value).then(|| id.clone()))
                })
                .flatten();
            let resolution = if variable_id.is_some() {
                "variable-token"
            } else if style_id.is_some() {
                "style-token"
            } else {
                "raw-fallback"
            };
            entries.push(generated_entry(
                range.start + open_relative + start,
                range.start + open_relative + end,
                &node_id,
                property,
                variable_id,
                style_id,
                resolution,
            ));
        }

        if node.node_type == "TEXT" {
            add_text_entries(
                &tsx,
                &range,
                &node_id,
                node.typed_view().string("characters").unwrap_or_default(),
                variable_tokens,
                style_tokens,
                &mut entries,
            );
        }
    }
    entries.sort_by(|left, right| {
        let left_range = left.generated_range.as_ref();
        let right_range = right.generated_range.as_ref();
        left_range
            .map(|range| (range.start, range.end))
            .cmp(&right_range.map(|range| (range.start, range.end)))
            .then_with(|| left.node_id.cmp(&right.node_id))
            .then_with(|| left.property.cmp(&right.property))
    });
    (
        tsx,
        SourceMap {
            version: 1,
            entries,
        },
    )
}

const PROP_SOURCES: &[(&str, &str)] = &[
    ("alignContent", "textAlignVertical"),
    ("alignItems", "counterAxisAlignItems"),
    ("aspectRatio", "targetAspectRatio"),
    ("bg", "fills"),
    ("border", "strokes"),
    ("borderBottom", "strokes"),
    ("borderLeft", "strokes"),
    ("borderRight", "strokes"),
    ("borderTop", "strokes"),
    ("borderRadius", "cornerRadius"),
    ("bottom", "y"),
    ("boxSize", "width"),
    ("color", "fills"),
    ("flex", "layoutGrow"),
    ("flexDir", "layoutMode"),
    ("fontFamily", "fontName"),
    ("fontSize", "fontSize"),
    ("fontStyle", "fontName"),
    ("fontWeight", "fontWeight"),
    ("gap", "itemSpacing"),
    ("h", "height"),
    ("justifyContent", "primaryAxisAlignItems"),
    ("left", "x"),
    ("letterSpacing", "letterSpacing"),
    ("lineHeight", "lineHeight"),
    ("m", "itemSpacing"),
    ("maxH", "maxHeight"),
    ("maxW", "maxWidth"),
    ("minH", "minHeight"),
    ("minW", "minWidth"),
    ("opacity", "opacity"),
    ("overflow", "clipsContent"),
    ("p", "paddingTop"),
    ("pb", "paddingBottom"),
    ("pl", "paddingLeft"),
    ("pos", "layoutPositioning"),
    ("pr", "paddingRight"),
    ("pt", "paddingTop"),
    ("px", "paddingLeft"),
    ("py", "paddingTop"),
    ("right", "x"),
    ("textAlign", "textAlignHorizontal"),
    ("top", "y"),
    ("typography", "textStyleId"),
    ("w", "width"),
    ("whiteSpace", "textAutoResize"),
    ("wordBreak", "characters"),
];

fn add_text_entries(
    tsx: &str,
    range: &GeneratedRange,
    node_id: &str,
    characters: &str,
    variable_tokens: &BTreeMap<String, String>,
    style_tokens: &BTreeMap<String, String>,
    entries: &mut Vec<ProvenanceEntry>,
) {
    let source = &tsx[range.start..range.end];
    if let Some(fragment) = characters
        .split('\n')
        .map(str::trim)
        .find(|fragment| !fragment.is_empty() && source.contains(fragment))
        && let Some(start) = source.find(fragment)
    {
        entries.push(generated_entry(
            range.start + start,
            range.start + start + fragment.len(),
            node_id,
            "characters",
            None,
            None,
            "exact",
        ));
    }
    for (variable_id, token) in variable_tokens {
        let needle = format!("${token}");
        for start in match_indices(source, &needle) {
            let prop_start = source[..start]
                .rfind(|character: char| character.is_whitespace() || character == '<')
                .map_or(start, |offset| offset + 1);
            let prop_end = source[start..]
                .find('"')
                .map_or(start + needle.len(), |offset| start + offset + 1);
            entries.push(generated_entry(
                range.start + prop_start,
                range.start + prop_end,
                node_id,
                "styledTextSegments",
                Some(variable_id.clone()),
                None,
                "variable-token",
            ));
        }
    }
    for (style_id, token) in style_tokens {
        let needle = format!("typography=\"{token}\"");
        for start in match_indices(source, &needle) {
            entries.push(generated_entry(
                range.start + start,
                range.start + start + needle.len(),
                node_id,
                "styledTextSegments",
                None,
                Some(style_id.clone()),
                "style-token",
            ));
        }
    }
}

fn find_resource_id(
    value: &str,
    tokens: &BTreeMap<String, String>,
    prefix: char,
) -> Option<String> {
    tokens.iter().find_map(|(id, token)| {
        let needle = format!("{prefix}{token}");
        match_indices(value, &needle)
            .into_iter()
            .any(|start| {
                value[start + needle.len()..]
                    .chars()
                    .next()
                    .is_none_or(|character| !is_token_character(character))
            })
            .then(|| id.clone())
    })
}

fn find_prop(opening: &str, needle: &str) -> Option<usize> {
    match_indices(opening, needle).into_iter().find(|start| {
        *start > 0
            && opening[..*start]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace)
    })
}

fn is_token_character(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '_' | '-')
}

fn match_indices(source: &str, needle: &str) -> Vec<usize> {
    let mut result = Vec::new();
    let mut offset = 0;
    while let Some(found) = source[offset..].find(needle) {
        let absolute = offset + found;
        result.push(absolute);
        offset = absolute + needle.len();
    }
    result
}

fn generated_entry(
    start: usize,
    end: usize,
    node_id: &str,
    property: &str,
    variable_id: Option<String>,
    style_id: Option<String>,
    resolution: &str,
) -> ProvenanceEntry {
    ProvenanceEntry {
        generated_range: Some(GeneratedRange { start, end }),
        json_pointer: None,
        node_id: Some(node_id.to_owned()),
        property: Some(property.to_owned()),
        variable_id,
        style_id,
        resolution: resolution.to_owned(),
    }
}

fn strip_markers(marked: &str) -> (String, Vec<(String, GeneratedRange)>) {
    let mut output = String::with_capacity(marked.len());
    let mut stack = Vec::<(String, usize)>::new();
    let mut ranges = Vec::new();
    let mut cursor = 0;
    while cursor < marked.len() {
        let rest = &marked[cursor..];
        let next_start = rest.find(START);
        let next_end = rest.find(END);
        let next = match (next_start, next_end) {
            (Some(start), Some(end)) => start.min(end),
            (Some(start), None) => start,
            (None, Some(end)) => end,
            (None, None) => {
                output.push_str(rest);
                break;
            }
        };
        output.push_str(&rest[..next]);
        cursor += next;
        let is_start = marked[cursor..].starts_with(START);
        let prefix = if is_start { START } else { END };
        cursor += prefix.len();
        let Some(close) = marked[cursor..].find(CLOSE) else {
            output.push_str(&marked[cursor - prefix.len()..]);
            break;
        };
        let node_id = marked[cursor..cursor + close].to_owned();
        cursor += close + CLOSE.len_utf8();
        if is_start {
            stack.push((node_id, output.len()));
        } else if let Some(index) = stack.iter().rposition(|(id, _)| id == &node_id) {
            let (_, start) = stack.remove(index);
            ranges.push((
                node_id,
                GeneratedRange {
                    start,
                    end: output.len(),
                },
            ));
        }
    }
    ranges.sort_by(|left, right| {
        left.1
            .start
            .cmp(&right.1.start)
            .then_with(|| right.1.end.cmp(&left.1.end))
            .then_with(|| left.0.cmp(&right.0))
    });
    (output, ranges)
}

pub(crate) fn json_pointer_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
