use std::collections::{BTreeMap, BTreeSet};

use devup_mcp_figma::TypedNode;
use serde_json::Value;

use super::{
    component::{Prop, PropValue, render_static_attribute},
    layout::{format_number, px, string_prop},
    style::first_solid_color,
};

pub(super) fn push_text_props(
    view: &TypedNode<'_>,
    text_style_tokens: &BTreeMap<String, String>,
    variable_tokens: &BTreeMap<String, String>,
    used_tokens: &mut BTreeSet<String>,
    props: &mut Vec<Prop>,
) {
    if view.node_type() != "TEXT" {
        return;
    }
    let segment = default_segment(view);
    let typography = segment
        .and_then(|value| value.get("textStyleId"))
        .and_then(Value::as_str)
        .and_then(|id| text_style_tokens.get(id));
    let value = |field: &str| {
        view.value(field)
            .filter(|value| is_resolved_value(value))
            .or_else(|| segment.and_then(|value| value.get(field)))
    };
    if let Some(color) = bound_segment_color(value("fills"), variable_tokens)
        .or_else(|| first_solid_color(value("fills")))
    {
        record_used_color(&color, used_tokens);
        replace_prop(props, "color", color);
    }
    if let Some(typography) = typography {
        string_prop(props, "typography", typography);
    } else if let Some(family) = value("fontName")
        .and_then(Value::as_object)
        .and_then(|font| font.get("family"))
        .and_then(Value::as_str)
    {
        string_prop(props, "fontFamily", family);
    }
    if typography.is_none()
        && value("fontName")
            .and_then(Value::as_object)
            .and_then(|font| font.get("style"))
            .and_then(Value::as_str)
            .is_some_and(|style| style.contains("Italic"))
    {
        string_prop(props, "fontStyle", "italic");
    }
    if typography.is_none()
        && let Some(font_size) = value("fontSize").and_then(Value::as_f64)
    {
        string_prop(props, "fontSize", px(font_size));
    }
    if typography.is_none()
        && let Some(weight) = value("fontWeight").and_then(Value::as_f64)
    {
        string_prop(props, "fontWeight", format_number(weight));
    }
    if typography.is_none()
        && let Some(letter_spacing) = letter_spacing(value("letterSpacing"))
    {
        string_prop(props, "letterSpacing", letter_spacing);
    }
    if typography.is_none()
        && let Some(line_height) = line_height(value("lineHeight"))
    {
        string_prop(props, "lineHeight", line_height);
    }
    if typography.is_none() {
        match value("textDecoration").and_then(Value::as_str) {
            Some("UNDERLINE") => string_prop(props, "textDecoration", "underline"),
            Some("STRIKETHROUGH") => string_prop(props, "textDecoration", "line-through"),
            _ => {}
        }
        if let Some(case) = value("textCase").and_then(Value::as_str)
            && case != "ORIGINAL"
        {
            string_prop(props, "textTransform", case.to_ascii_lowercase());
        }
    }
    if let Some(max_lines) = view.number("maxLines") {
        if max_lines == 1.0 {
            string_prop(props, "whiteSpace", "nowrap");
        } else if max_lines > 1.0 {
            string_prop(props, "WebkitBoxOrient", "vertical");
            string_prop(props, "WebkitLineClamp", format_number(max_lines));
            string_prop(props, "display", "-webkit-box");
        }
    }
    // Reads the designer's own truncation setting, which Figma always
    // reports — provided it is collected. It was missing from the field
    // manifest, so this saw nothing and every text claimed an ellipsis the
    // design never asked for.
    if view.string("textTruncation") != Some("DISABLED")
        && view.string("layoutSizingHorizontal") != Some("HUG")
    {
        string_prop(props, "overflow", "hidden");
        string_prop(props, "textOverflow", "ellipsis");
    }
    let horizontal_hug = view
        .string("textAutoResize")
        .is_some_and(|value| value.contains("WIDTH"));
    let single_line = !view.string("characters").unwrap_or_default().contains('\n');
    if !(horizontal_hug && single_line)
        && let Some(alignment) = view.string("textAlignHorizontal")
    {
        let value = match alignment {
            "RIGHT" => Some("right"),
            "CENTER" => Some("center"),
            "JUSTIFIED" => Some("justify"),
            _ => None,
        };
        if let Some(value) = value {
            string_prop(props, "textAlign", value);
        }
    }
    let vertical_hug = view
        .string("textAutoResize")
        .is_some_and(|value| value.contains("HEIGHT"));
    if !vertical_hug {
        match view.string("textAlignVertical") {
            Some("CENTER") => string_prop(props, "alignContent", "center"),
            Some("BOTTOM") => string_prop(props, "alignContent", "end"),
            _ => {}
        }
    }
    if segments_contain_korean(view) {
        string_prop(props, "wordBreak", "keep-all");
    }
    if let Some(strokes) = view.value("strokes")
        && let Some(color) = first_solid_color(Some(strokes))
    {
        string_prop(
            props,
            "WebkitTextStroke",
            format!("{} {color}", px(view.number("strokeWeight").unwrap_or(1.0))),
        );
        string_prop(props, "paintOrder", "stroke fill");
    }
    if let Some(list) = segment
        .and_then(|value| value.get("listOptions"))
        .and_then(Value::as_object)
        .and_then(|value| value.get("type"))
        .and_then(Value::as_str)
    {
        match list {
            "UNORDERED" => string_prop(props, "as", "ul"),
            "ORDERED" => string_prop(props, "as", "ol"),
            _ => return,
        }
        string_prop(props, "my", "0px");
        string_prop(props, "pl", "1.5em");
    }
}

fn is_resolved_value(value: &Value) -> bool {
    !value.as_object().is_some_and(|object| {
        ["$unsupported", "$undefined", "$truncated", "$error"]
            .iter()
            .any(|marker| object.contains_key(*marker))
    })
}

fn default_segment<'a>(view: &'a TypedNode<'a>) -> Option<&'a Value> {
    let segments = view.value("styledTextSegments")?.as_array()?;
    let mut selected = segments.first()?;
    let mut longest = selected
        .get("characters")
        .and_then(Value::as_str)
        .map(str::len)
        .unwrap_or_default();
    for segment in segments.iter().skip(1) {
        let length = segment
            .get("characters")
            .and_then(Value::as_str)
            .map(str::len)
            .unwrap_or_default();
        if length >= longest {
            selected = segment;
            longest = length;
        }
    }
    Some(selected)
}

fn letter_spacing(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if let Some(number) = value.as_f64() {
        return Some(px(number));
    }
    let object = value.as_object()?;
    let number = object.get("value").and_then(Value::as_f64).unwrap_or(0.0);
    match object.get("unit").and_then(Value::as_str) {
        Some("PERCENT") => Some(format!("{}em", format_number(number.round() / 100.0))),
        _ => Some(px(number)),
    }
}

fn line_height(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if let Some(number) = value.as_f64() {
        return Some(px(number));
    }
    let object = value.as_object()?;
    match object.get("unit").and_then(Value::as_str) {
        Some("AUTO") => Some("normal".to_owned()),
        Some("PERCENT") => object
            .get("value")
            .and_then(Value::as_f64)
            .map(|number| format_number((number / 10.0).round() / 10.0)),
        _ => object.get("value").and_then(Value::as_f64).map(px),
    }
}

fn segments_contain_korean(view: &TypedNode<'_>) -> bool {
    view.value("styledTextSegments")
        .and_then(Value::as_array)
        .is_some_and(|segments| {
            segments.iter().any(|segment| {
                segment
                    .get("characters")
                    .and_then(Value::as_str)
                    .is_some_and(|text| {
                        text.chars().any(|character| {
                            matches!(
                                character as u32,
                                0xAC00..=0xD7AF | 0x1100..=0x11FF | 0x3130..=0x318F
                            )
                        })
                    })
            })
        })
}

fn replace_prop(props: &mut Vec<Prop>, name: &str, value: String) {
    if let Some((_, existing)) = props.iter_mut().find(|(prop, _)| prop == name) {
        *existing = PropValue::String(value);
    } else {
        string_prop(props, name, value);
    }
}

pub(super) fn render_text_children(
    view: &TypedNode<'_>,
    text_style_tokens: &BTreeMap<String, String>,
    variable_tokens: &BTreeMap<String, String>,
    used_tokens: &mut BTreeSet<String>,
    depth: usize,
) -> String {
    let indent = "  ".repeat(depth);
    let Some(segments) = view.value("styledTextSegments").and_then(Value::as_array) else {
        return format!(
            "{indent}{}",
            escape_jsx_text(view.string("characters").unwrap_or_default())
        );
    };
    if segments.is_empty() {
        return indent;
    }
    let default = default_segment(view).expect("non-empty styledTextSegments");
    let default_props = typography_props(default, text_style_tokens, variable_tokens, used_tokens);
    let mut rendered = Vec::new();
    for segment in segments {
        let value = segment
            .get("characters")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let list = segment
            .get("listOptions")
            .and_then(Value::as_object)
            .and_then(|value| value.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("NONE");
        if list != "NONE" {
            rendered.extend(value.lines().map(|line| {
                format!(
                    "{indent}<li>\n{}{}\n{indent}</li>",
                    "  ".repeat(depth + 1),
                    escape_jsx_text(line)
                )
            }));
            continue;
        }

        let mut segment_props =
            typography_props(segment, text_style_tokens, variable_tokens, used_tokens);
        segment_props.retain(|(name, value)| {
            !default_props
                .iter()
                .any(|(default_name, default_value)| default_name == name && default_value == value)
        });
        let content = escape_jsx_text(value);
        if segment_props.is_empty() {
            rendered.push(format!("{indent}{content}"));
        } else {
            segment_props.sort_by(|left, right| left.0.cmp(&right.0));
            let props = segment_props
                .into_iter()
                .map(|(name, value)| match value {
                    PropValue::String(value) => render_static_attribute(&name, &value),
                })
                .collect::<Vec<_>>()
                .join(" ");
            rendered.push(format!(
                "{indent}<Text {props}>\n{}{}\n{indent}</Text>",
                "  ".repeat(depth + 1),
                content
            ));
        }
    }
    rendered.join("\n")
}

fn typography_props(
    segment: &Value,
    text_style_tokens: &BTreeMap<String, String>,
    variable_tokens: &BTreeMap<String, String>,
    used_tokens: &mut BTreeSet<String>,
) -> Vec<Prop> {
    let mut props = Vec::new();
    if let Some(color) = bound_segment_color(segment.get("fills"), variable_tokens)
        .or_else(|| first_solid_color(segment.get("fills")))
    {
        record_used_color(&color, used_tokens);
        string_prop(&mut props, "color", color);
    }
    let typography = segment
        .get("textStyleId")
        .and_then(Value::as_str)
        .and_then(|id| text_style_tokens.get(id));
    if let Some(typography) = typography {
        string_prop(&mut props, "typography", typography);
    } else if let Some(family) = segment
        .get("fontName")
        .and_then(Value::as_object)
        .and_then(|font| font.get("family"))
        .and_then(Value::as_str)
    {
        string_prop(&mut props, "fontFamily", family);
    }
    if typography.is_none()
        && segment
            .get("fontName")
            .and_then(Value::as_object)
            .and_then(|font| font.get("style"))
            .and_then(Value::as_str)
            .is_some_and(|style| style.contains("Italic"))
    {
        string_prop(&mut props, "fontStyle", "italic");
    }
    if typography.is_none()
        && let Some(value) = segment.get("fontSize").and_then(Value::as_f64)
    {
        string_prop(&mut props, "fontSize", px(value));
    }
    if typography.is_none()
        && let Some(value) = segment.get("fontWeight").and_then(Value::as_f64)
    {
        string_prop(&mut props, "fontWeight", format_number(value));
    }
    if typography.is_none()
        && let Some(value) = letter_spacing(segment.get("letterSpacing"))
    {
        string_prop(&mut props, "letterSpacing", value);
    }
    if typography.is_none()
        && let Some(value) = line_height(segment.get("lineHeight"))
    {
        string_prop(&mut props, "lineHeight", value);
    }
    if typography.is_none() {
        match segment.get("textDecoration").and_then(Value::as_str) {
            Some("UNDERLINE") => string_prop(&mut props, "textDecoration", "underline"),
            Some("STRIKETHROUGH") => string_prop(&mut props, "textDecoration", "line-through"),
            _ => {}
        }
        if let Some(case) = segment.get("textCase").and_then(Value::as_str)
            && case != "ORIGINAL"
        {
            string_prop(&mut props, "textTransform", case.to_ascii_lowercase());
        }
    }
    props
}

fn record_used_color(color: &str, used_tokens: &mut BTreeSet<String>) {
    if let Some(token) = color.strip_prefix('$') {
        used_tokens.insert(token.to_owned());
    }
}

fn bound_segment_color(
    fills: Option<&Value>,
    variable_tokens: &BTreeMap<String, String>,
) -> Option<String> {
    let id = fills?.as_array()?.iter().find_map(|paint| {
        paint
            .get("boundVariables")?
            .get("color")?
            .get("id")?
            .as_str()
    })?;
    variable_tokens.get(id).map(|token| format!("${token}"))
}

pub(super) fn escape_jsx_text(input: &str) -> String {
    let leading = input
        .chars()
        .take_while(|character| *character == ' ')
        .count();
    let trailing = input
        .chars()
        .rev()
        .take_while(|character| *character == ' ')
        .count();
    let middle_end = input.len().saturating_sub(trailing);
    let middle = &input[leading..middle_end];
    let mut result = String::new();
    if leading > 0 {
        result.push_str(&format!("{{\"{}\"}}", " ".repeat(leading)));
    }
    let mut characters = middle.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '{' => result.push_str("{\"{\"}"),
            '}' => result.push_str("{\"}\"}"),
            '&' => result.push_str("{\"&\"}"),
            '<' => result.push_str("{\"<\"}"),
            '>' => result.push_str("{\">\"}"),
            '\'' => result.push_str("{\"'\"}"),
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                if characters.peek().is_none() {
                    result.push_str("{\" \"}");
                } else {
                    result.push_str("<br />");
                }
            }
            '\n' => {
                if characters.peek().is_none() {
                    result.push_str("{\" \"}");
                } else {
                    result.push_str("<br />");
                }
            }
            value => result.push(value),
        }
    }
    if trailing > 0 {
        result.push_str(&format!("{{\"{}\"}}", " ".repeat(trailing)));
    }
    result
}
