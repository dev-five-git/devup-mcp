use std::collections::BTreeSet;

use serde_json::Value;

use crate::figma::TypedNode;

pub(super) fn push_style_props(
    view: &TypedNode<'_>,
    component: &str,
    props: &mut Vec<String>,
    used_tokens: &mut BTreeSet<String>,
) {
    let color_prop = if component == "Text" { "color" } else { "bg" };
    if let Some(token) = view
        .value("devupTokens")
        .and_then(Value::as_object)
        .and_then(|tokens| tokens.get("fills"))
        .and_then(Value::as_str)
    {
        used_tokens.insert(token.to_owned());
        props.push(format!("{color_prop}=\"${token}\""));
    } else if let Some(color) = first_solid_color(view.value("fills")) {
        props.push(format!("{color_prop}=\"{color}\""));
    }
    if let Some(opacity) = view.number("opacity")
        && opacity < 1.0
    {
        props.push(format!("opacity={{{opacity}}}"));
    }
}

fn first_solid_color(value: Option<&Value>) -> Option<String> {
    let paint = value?
        .as_array()?
        .iter()
        .find(|paint| paint["type"] == "SOLID")?;
    let color = paint.get("color")?;
    let channel = |name: &str| -> Option<u8> {
        let value = color.get(name)?.as_f64()?;
        Some((value.clamp(0.0, 1.0) * 255.0).round() as u8)
    };
    Some(format!(
        "#{:02x}{:02x}{:02x}",
        channel("r")?,
        channel("g")?,
        channel("b")?
    ))
}
