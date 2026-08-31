use std::collections::BTreeMap;

use devup_mcp_figma::Snapshot;
use serde::{Deserialize, Serialize};

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
