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
    if let Some(font_size) = value("fontSize").and_then(Value::as_f64) {
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
    if let Some(line_height) = line_height(
        value("lineHeight"),
        value("fontSize").and_then(Value::as_f64),
    ) {
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

fn line_height(value: Option<&Value>, font_size: Option<f64>) -> Option<String> {
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
            .zip(font_size)
            .map(|(number, size)| px((size * number / 100.0).round())),
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
    let Some(segments) = view
        .value("styledTextSegments")
        .and_then(Value::as_array)
        .filter(|segments| !segments.is_empty())
    else {
        return format!(
            "{indent}{}",
            escape_jsx_text(view.string("characters").unwrap_or_default())
        );
    };
    let default = default_segment(view).expect("non-empty styledTextSegments");
    let default_props = typography_props(default, text_style_tokens, variable_tokens, used_tokens);
    let mut rendered: Vec<String> = Vec::new();
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
            let lines = value
                .replace("\r\n", "\n")
                .replace(['\r', '\u{2028}', '\u{2029}'], "\n");
            let items = lines.split_terminator('\n').collect::<Vec<_>>();
            rendered.extend(items.iter().enumerate().map(|(index, line)| {
                // A trailing separator is a break inside the last item, not
                // another item (which would introduce an extra list marker).
                let trailing_break = if index + 1 == items.len() && lines.ends_with('\n') {
                    "<br />"
                } else {
                    ""
                };
                format!(
                    "{indent}<li>\n{}{}{trailing_break}\n{indent}</li>",
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
            if content.is_empty() {
                continue;
            }
            // Two bare children are one JSXText token. A formatting newline
            // between nonempty lines becomes a space, even inside a word.
            // Join only that hazardous boundary; element/expression boundaries
            // already prevent JSX from inserting a space and stay unchanged.
            if !content.starts_with(['<', '{'])
                && let Some(previous) = rendered.last_mut()
                && !previous.ends_with(['>', '}'])
            {
                previous.push_str(&content);
            } else {
                rendered.push(format!("{indent}{content}"));
            }
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
    if let Some(value) = segment.get("fontSize").and_then(Value::as_f64) {
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
    if let Some(value) = line_height(
        segment.get("lineHeight"),
        segment.get("fontSize").and_then(Value::as_f64),
    ) {
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

/// Token names carry no metrics. Resolved overrides must travel as a pair;
/// otherwise a pixel advance from the token may belong to another font size.
pub(super) fn validate_line_metrics(
    view: &TypedNode<'_>,
    tokens: &BTreeMap<String, String>,
) -> Result<(), devup_mcp_figma::DevupError> {
    if view.node_type() != "TEXT" {
        return Ok(());
    }
    let validate = |size: Option<&Value>,
                    height: Option<&Value>,
                    bound: Option<&Value>,
                    styled: bool| {
        let percent = height.and_then(|h| h.get("unit")).and_then(Value::as_str) == Some("PERCENT");
        let invalid = (percent
            && (size.and_then(Value::as_f64).is_none()
                || bound.and_then(|v| v.get("fontSize")).is_some()))
            || (styled
                && size.is_some()
                && line_height(height, size.and_then(Value::as_f64)).is_none());
        if invalid {
            Err(devup_mcp_figma::DevupError::new(
                devup_mcp_figma::ErrorCode::DevupCodegenFailed,
                format!(
                    "Text node '{}' cannot represent a size-dependent line advance without resolved fontSize and lineHeight; variable sizes require mode-aware metrics.",
                    view.id()
                ),
                false,
            ))
        } else {
            Ok(())
        }
    };
    let segment = default_segment(view);
    let value = |field: &str| {
        view.value(field)
            .filter(|v| is_resolved_value(v))
            .or_else(|| segment.and_then(|s| s.get(field)))
    };
    let styled = segment
        .and_then(|s| s.get("textStyleId"))
        .and_then(Value::as_str)
        .is_some_and(|id| tokens.contains_key(id));
    validate(
        value("fontSize"),
        value("lineHeight"),
        value("boundVariables"),
        styled,
    )?;
    for segment in view
        .value("styledTextSegments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let styled = segment
            .get("textStyleId")
            .and_then(Value::as_str)
            .is_some_and(|id| tokens.contains_key(id));
        validate(
            segment.get("fontSize"),
            segment.get("lineHeight"),
            segment.get("boundVariables"),
            styled,
        )?;
    }
    Ok(())
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

/// Whether a character is whitespace to a JavaScript regex's `\s`.
///
/// Wider than ASCII: it takes in the no-break space, the Unicode spaces and
/// the line and paragraph separators. The last two matter — Figma writes a
/// soft return as U+2028, and a run of them at the edge of a segment is
/// whitespace to the plugin.
fn is_js_whitespace(character: char) -> bool {
    matches!(
        character,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// Preserve ordinary text in the plugin's JSX spelling. Edge spaces use
/// string expressions because source indentation would trim them; other edge
/// whitespace retains its actual characters instead of becoming ASCII spaces.
/// Design line separators paint as explicit breaks (CRLF is one separator).
/// This encoder is also used by the provenance mapper.
pub(crate) fn escape_jsx_text(input: &str) -> String {
    let characters = input.chars().collect::<Vec<_>>();
    let leading = characters
        .iter()
        .take_while(|character| is_js_whitespace(**character))
        .count();
    let trailing = if leading == characters.len() {
        0
    } else {
        characters
            .iter()
            .rev()
            .take_while(|character| is_js_whitespace(**character))
            .count()
    };
    let middle = &characters[leading..characters.len() - trailing];

    let mut result = String::new();
    push_edge_whitespace(&characters[..leading], &mut result);
    let mut index = 0;
    while index < middle.len() {
        let character = middle[index];
        match character {
            '{' | '}' | '&' | '<' | '>' | '\'' => {
                let run_end = middle[index..]
                    .iter()
                    .position(|other| !matches!(other, '{' | '}' | '&' | '<' | '>' | '\''))
                    .map_or(middle.len(), |offset| index + offset);
                let run = middle[index..run_end].iter().collect::<String>();
                result.push_str(&format!("{{\"{run}\"}}"));
                index = run_end;
                continue;
            }
            '\r' => {
                if middle.get(index + 1) == Some(&'\n') {
                    index += 1;
                }
                result.push_str("<br />");
            }
            '\n' | '\u{2028}' | '\u{2029}' => result.push_str("<br />"),
            '\t' => result.push_str("{\"\\t\"}"),
            other => result.push(other),
        }
        index += 1;
    }
    push_edge_whitespace(&characters[characters.len() - trailing..], &mut result);
    result
}

/// Preserve each whitespace run verbatim, with explicit design line breaks.
fn push_edge_whitespace(run: &[char], into: &mut String) {
    let mut start = 0;
    while start < run.len() {
        if matches!(run[start], '\r' | '\n' | '\u{2028}' | '\u{2029}') {
            if run[start] == '\r' && run.get(start + 1) == Some(&'\n') {
                start += 1;
            }
            into.push_str("<br />");
            start += 1;
        } else {
            let end = run[start..]
                .iter()
                .position(|ch| matches!(ch, '\r' | '\n' | '\u{2028}' | '\u{2029}'))
                .map_or(run.len(), |offset| start + offset);
            let whitespace = run[start..end].iter().collect::<String>();
            into.push('{');
            into.push_str(&serde_json::to_string(&whitespace).expect("serialize whitespace"));
            into.push('}');
            start = end;
        }
    }
}

/// CSS normal and nowrap both collapse ASCII spaces/tabs; JSX expressions do
/// not prevent that. Report the remaining visual loss without changing layout.
pub(super) fn whitespace_collapse_diagnostic(
    view: &TypedNode<'_>,
) -> Option<devup_mcp_figma::Diagnostic> {
    if view.node_type() != "TEXT" {
        return None;
    }
    let characters = view
        .string("characters")
        .map(str::to_owned)
        .unwrap_or_else(|| {
            view.value("styledTextSegments")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|segment| segment.get("characters").and_then(Value::as_str))
                .collect()
        });
    let collapses = characters
        .split(['\r', '\n', '\u{2028}', '\u{2029}'])
        .any(|line| {
            line.starts_with(' ')
                || line.ends_with(' ')
                || line.contains("  ")
                || line.contains('\t')
        });
    if !collapses {
        return None;
    }
    Some(devup_mcp_figma::Diagnostic {
        code: "DEVUP_CODEGEN_TEXT_WHITESPACE_COLLAPSE".into(),
        node_id: Some(view.id().to_owned()),
        property: Some("characters".into()),
        message: "Text characters are retained in JSX, but CSS collapses source spaces or tabs."
            .into(),
        fidelity_impact: Some(devup_mcp_figma::FidelityImpact::Lossy),
        details: Some(serde_json::json!({
            "originalValue": characters,
            "classification": "css-whitespace-collapse",
            "whiteSpace": if view.number("maxLines") == Some(1.0) { "nowrap" } else { "normal" },
            "nextAction": "Review intentional spaces or tabs in Figma and choose an explicit whitespace-preserving style if required."
        })),
        ..devup_mcp_figma::Diagnostic::default()
    })
}
