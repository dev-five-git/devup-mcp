use serde_json::Value;

use devup_mcp_figma::TypedNode;

use super::layout::px;

pub(super) fn push_text_props(view: &TypedNode<'_>, props: &mut Vec<String>) {
    if view.node_type() != "TEXT" {
        return;
    }
    if let Some(font_size) = view.number("fontSize") {
        props.push(format!("fontSize=\"{}\"", px(font_size)));
    }
    if let Some(line_height) = view
        .value("lineHeight")
        .and_then(Value::as_object)
        .and_then(|line_height| line_height.get("value"))
        .and_then(Value::as_f64)
    {
        props.push(format!("lineHeight=\"{}\"", px(line_height)));
    }
}

pub(super) fn escape_jsx_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('{', "&#123;")
        .replace('}', "&#125;")
}
