use quick_xml::{Reader, XmlVersion, events::Event};
use serde::Deserialize;
use serde_json::Value;

use crate::{DevupError, ErrorCode, UpstreamResult};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataDocument {
    pub file_key: String,
    #[serde(default)]
    pub version: Option<String>,
    pub root_id: String,
    #[serde(default)]
    pub nodes: Vec<MetadataNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub children_ids: Vec<String>,
    #[serde(default)]
    pub descendant_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataResult {
    Document(MetadataDocument),
    TopLevelPages(Vec<MetadataNode>),
}

impl MetadataDocument {
    pub fn root(&self) -> Option<&MetadataNode> {
        self.nodes.iter().find(|node| node.id == self.root_id)
    }
}

pub fn metadata_from_result_for_target(
    result: &UpstreamResult,
    expected_file_key: &str,
    expected_root_id: Option<&str>,
) -> Result<MetadataResult, DevupError> {
    find_metadata(&result.raw)
        .map(MetadataResult::Document)
        .or_else(|| {
            find_xml_metadata(&result.raw, expected_file_key, expected_root_id)
                .map(MetadataResult::Document)
        })
        .or_else(|| find_top_level_pages(&result.raw).map(MetadataResult::TopLevelPages))
        .ok_or_else(|| {
            DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "metadata not found in the Figma MCP response.",
                false,
            )
        })
}

fn find_top_level_pages(value: &Value) -> Option<Vec<MetadataNode>> {
    match value {
        Value::Object(object) => object.values().find_map(find_top_level_pages),
        Value::Array(values) => values.iter().find_map(find_top_level_pages),
        Value::String(text) => parse_top_level_pages(text),
        _ => None,
    }
}

fn parse_top_level_pages(text: &str) -> Option<Vec<MetadataNode>> {
    let (_, list) = text.split_once("Top-level pages of the document:")?;
    let pages = list
        .lines()
        .filter_map(|line| {
            let item = line.trim().strip_prefix("- ")?;
            let id_colon = item.find(':')?;
            let separator = item[id_colon + 1..].find(": ")? + id_colon + 1;
            let id = &item[..separator];
            let name = &item[separator + 2..];
            let (major, minor) = id.split_once(':')?;
            if major.is_empty()
                || minor.is_empty()
                || !major.bytes().all(|byte| byte.is_ascii_digit())
                || !minor.bytes().all(|byte| byte.is_ascii_digit())
                || name.is_empty()
            {
                return None;
            }
            Some(MetadataNode {
                id: id.to_owned(),
                node_type: "PAGE".to_owned(),
                name: Some(name.to_owned()),
                children_ids: Vec::new(),
                descendant_count: 0,
            })
        })
        .collect::<Vec<_>>();
    (!pages.is_empty()).then_some(pages)
}

#[derive(Debug)]
struct XmlNode {
    id: String,
    node_type: String,
    name: Option<String>,
    children: Vec<usize>,
}

fn find_xml_metadata(
    value: &Value,
    expected_file_key: &str,
    expected_root_id: Option<&str>,
) -> Option<MetadataDocument> {
    match value {
        Value::Object(object) => object
            .values()
            .find_map(|value| find_xml_metadata(value, expected_file_key, expected_root_id)),
        Value::Array(values) => values
            .iter()
            .find_map(|value| find_xml_metadata(value, expected_file_key, expected_root_id)),
        Value::String(text) => xml_slice(text)
            .and_then(|xml| parse_xml_metadata(xml, expected_file_key, expected_root_id)),
        _ => None,
    }
}

/// Extracts the XML region from a `get_metadata` text response.
///
/// Figma no longer returns bare XML. When the user has the queried node
/// selected in the desktop app, the response is *prepended* with a
/// `Currently selected nodes:` block, and every response is *appended* with
/// an `IMPORTANT: After you call this tool...` instruction footer. Requiring
/// the text to start with `<` therefore made devup-mcp fail with
/// `metadata not found in the Figma MCP response.` for the very common case
/// of "the user is looking at the node they asked about".
///
/// Slicing between the first `<` and the last `>` keeps the pre-existing
/// bare-XML input working unchanged, tolerates prose on either side, and
/// still yields `None` for text that carries no element at all. Text that
/// merely *contains* angle brackets is not a risk: `parse_xml_metadata`
/// returns `None` unless it finds at least one element with an `id`.
fn xml_slice(text: &str) -> Option<&str> {
    let start = text.find('<')?;
    let end = text.rfind('>')?;
    (end > start).then(|| &text[start..=end])
}

fn parse_xml_metadata(
    text: &str,
    expected_file_key: &str,
    expected_root_id: Option<&str>,
) -> Option<MetadataDocument> {
    if expected_file_key.is_empty() {
        return None;
    }
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut nodes = Vec::<XmlNode>::new();
    let mut stack = Vec::<Option<usize>>::new();

    loop {
        match reader.read_event().ok()? {
            Event::Start(element) => {
                let index = push_xml_node(&element, &mut nodes, stack.last().copied().flatten());
                stack.push(index);
            }
            Event::Empty(element) => {
                push_xml_node(&element, &mut nodes, stack.last().copied().flatten());
            }
            Event::End(_) => {
                stack.pop();
            }
            Event::Eof => break,
            _ => {}
        }
    }

    if nodes.is_empty() {
        return None;
    }
    let root_index = expected_root_id
        .and_then(|expected| nodes.iter().position(|node| node.id == expected))
        .unwrap_or(0);
    let root_id = nodes[root_index].id.clone();
    let metadata_nodes = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| MetadataNode {
            id: node.id.clone(),
            node_type: node.node_type.clone(),
            name: node.name.clone(),
            children_ids: node
                .children
                .iter()
                .map(|child| nodes[*child].id.clone())
                .collect(),
            descendant_count: descendant_count(index, &nodes),
        })
        .collect();
    Some(MetadataDocument {
        file_key: expected_file_key.to_owned(),
        version: None,
        root_id,
        nodes: metadata_nodes,
    })
}

fn push_xml_node(
    element: &quick_xml::events::BytesStart<'_>,
    nodes: &mut Vec<XmlNode>,
    parent: Option<usize>,
) -> Option<usize> {
    let mut id = None;
    let mut name = None;
    for attribute in element.attributes().flatten() {
        let key = attribute.key.as_ref();
        if key == "id" || key == "name" {
            let value = attribute
                .normalized_value(XmlVersion::Implicit1_0)
                .ok()?
                .into_owned();
            if key == "id" {
                id = Some(value);
            } else {
                name = Some(value);
            }
        }
    }
    let id = id?;
    let index = nodes.len();
    nodes.push(XmlNode {
        id,
        node_type: element
            .name()
            .as_ref()
            .replace('-', "_")
            .to_ascii_uppercase(),
        name,
        children: Vec::new(),
    });
    if let Some(parent) = parent {
        nodes[parent].children.push(index);
    }
    Some(index)
}

fn descendant_count(index: usize, nodes: &[XmlNode]) -> usize {
    1 + nodes[index]
        .children
        .iter()
        .map(|child| descendant_count(*child, nodes))
        .sum::<usize>()
}

fn find_metadata(value: &Value) -> Option<MetadataDocument> {
    if let Ok(document) = serde_json::from_value::<MetadataDocument>(value.clone())
        && !document.file_key.is_empty()
        && !document.root_id.is_empty()
    {
        return Some(document);
    }
    match value {
        Value::Object(object) => {
            if let Some(Value::String(text)) = object.get("text")
                && let Ok(parsed) = serde_json::from_str::<Value>(text)
                && let Some(document) = find_metadata(&parsed)
            {
                return Some(document);
            }
            object.values().find_map(find_metadata)
        }
        Value::Array(values) => values.iter().find_map(find_metadata),
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|parsed| find_metadata(&parsed)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = "<frame id=\"3997:48764\" name=\"A : STORY-INTRO\">\n  \
                       <text id=\"3997:48777\" name=\"Prompt\" />\n</frame>";

    fn parse(text: &str) -> Option<MetadataDocument> {
        find_xml_metadata(
            &Value::String(text.to_owned()),
            "85CgSws3o5XsLv7aAwWJyS",
            Some("3997:48764"),
        )
    }

    #[test]
    fn bare_xml_still_parses() {
        let document = parse(XML).expect("bare XML");
        assert_eq!(document.root_id, "3997:48764");
        assert_eq!(document.nodes.len(), 2);
    }

    /// The regression this fix exists for: with the node selected in the
    /// Figma desktop app, `get_metadata` prepends a selection block, which
    /// used to make the whole legacy metadata path fail.
    #[test]
    fn a_selected_nodes_preamble_is_tolerated() {
        let text = format!("Currently selected nodes:\n- 3997:48764: A : STORY-INTRO\n\n\n\n{XML}");
        let document = parse(&text).expect("preamble must not break parsing");
        assert_eq!(document.root_id, "3997:48764");
        assert_eq!(document.nodes.len(), 2);
    }

    #[test]
    fn an_instruction_footer_is_tolerated() {
        let text = format!(
            "{XML}\n\nIMPORTANT: After you call this tool, you MUST call get_design_context \
             if trying to implement the design."
        );
        assert_eq!(parse(&text).expect("footer").root_id, "3997:48764");
    }

    #[test]
    fn a_preamble_and_a_footer_together_are_tolerated() {
        let text =
            format!("Currently selected nodes:\n- 3997:48764: A\n\n{XML}\n\nIMPORTANT: do X.");
        let document = parse(&text).expect("preamble and footer");
        assert_eq!(document.root_id, "3997:48764");
        assert_eq!(document.nodes.len(), 2);
    }

    #[test]
    fn prose_without_any_element_is_still_rejected() {
        assert!(parse("Currently selected nodes:\n- 3997:48764: A : STORY-INTRO").is_none());
        assert!(parse("no angle brackets here at all").is_none());
        // Angle brackets but no element carrying an `id`.
        assert!(parse("a < b and c > d").is_none());
    }
}
