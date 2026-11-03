//! Non-visual HTML semantics backed by explicit layer/component names.
//!
//! A canvas or a large font is not evidence of a main region or a heading.
//! Unnamed content still needs the consumer's accessibility review.

use devup_mcp_figma::{RawNode, Snapshot};

use super::{component::Prop, layout::string_prop};

pub(super) fn scalar_html_value(value: &str) -> String {
    format!(
        "=\"{}\"",
        value
            .replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    )
}

fn semantic_name<'a>(snapshot: &'a Snapshot, node: &'a RawNode) -> &'a str {
    let view = node.typed_view();
    // Variant roots are commonly called `Viewport=desktop`, while the set
    // holds the meaningful component name (Header, Footer, ...).
    if node.node_type == "COMPONENT"
        && let Some(parent) = view
            .string("parentId")
            .and_then(|id| snapshot.nodes.get(id))
        && parent.node_type == "COMPONENT_SET"
    {
        return parent.typed_view().name().unwrap_or_default();
    }
    view.name().unwrap_or_default()
}

fn key(name: &str) -> String {
    name.split('/')
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub(super) fn push_semantic_props(
    snapshot: &Snapshot,
    node: &RawNode,
    component: &str,
    props: &mut Vec<Prop>,
) {
    let name = semantic_name(snapshot, node);
    if component == "Image" {
        // Background paint layers already use alt="". For an ordinary asset,
        // preserve the designer's descriptive layer name;
        // generated shape/icon names would only add screen-reader noise.
        let view = node.typed_view();
        let layer = view.name().unwrap_or_default().trim();
        let generic = [
            "rectangle",
            "ellipse",
            "vector",
            "polygon",
            "star",
            "line",
            "union",
            "subtract",
            "intersect",
            "exclude",
            "image",
            "icon",
        ]
        .iter()
        .any(|prefix| {
            let lower = layer.to_ascii_lowercase();
            lower == *prefix
                || lower.strip_prefix(prefix).is_some_and(|suffix| {
                    suffix.starts_with([' ', '-', '_', '/'])
                        && (*prefix == "icon"
                            || suffix.chars().all(|character| {
                                character.is_ascii_digit()
                                    || matches!(character, ' ' | '-' | '_' | '/')
                            }))
                })
        });
        if !props.iter().any(|(name, _)| name == "alt") {
            string_prop(props, "alt", if generic { "" } else { layer });
        }
        return;
    }
    if props.iter().any(|(name, _)| name == "as") {
        return;
    }
    let tag = if component == "Text" {
        match key(name).as_str() {
            "h1" | "heading1" | "pageheading" | "pagetitle" => Some("h1"),
            "h2" | "heading2" => Some("h2"),
            "h3" | "heading3" => Some("h3"),
            "h4" | "heading4" => Some("h4"),
            "h5" | "heading5" => Some("h5"),
            "h6" | "heading6" => Some("h6"),
            _ => None,
        }
    } else if matches!(
        node.node_type.as_str(),
        "FRAME" | "COMPONENT" | "INSTANCE" | "GROUP"
    ) {
        match key(name).as_str() {
            "header" | "siteheader" | "globalheader" => Some("header"),
            "footer" | "sitefooter" | "globalfooter" => Some("footer"),
            "nav" | "navigation" | "gnb" | "globalnavigation" | "primarynavigation" => Some("nav"),
            "maincontent" | "pagecontent" => Some("main"),
            _ => None,
        }
    } else {
        None
    };
    if let Some(tag) = tag {
        string_prop(props, "as", tag);
    }
}
