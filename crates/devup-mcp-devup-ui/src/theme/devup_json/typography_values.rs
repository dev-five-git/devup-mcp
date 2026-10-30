// A missing size is representable (including percentage line height). An
// unreadable style or malformed metric is not; isolate it rather than invent
// a default typography entry or reject all the other resources.
fn typography_problem(style: &Value) -> Option<String> {
    let Some(object) = style.as_object() else {
        return Some("style value is not an object".into());
    };
    fn unreadable(value: &Value) -> bool {
        match value {
            Value::Object(object) => {
                ["$unsupported", "$undefined", "$truncated", "$error"]
                    .iter()
                    .any(|marker| object.contains_key(*marker))
                    || object.values().any(unreadable)
            }
            Value::Array(values) => values.iter().any(unreadable),
            _ => false,
        }
    }
    if unreadable(style) || object.values().any(unreadable) {
        return Some("style contains an unreadable field; recollect the style".into());
    }
    for metric in ["lineHeight", "letterSpacing"] {
        if let Some(value) = object.get(metric).filter(|v| !v.is_null()) {
            let unit = value.get("unit").and_then(Value::as_str);
            if unit != Some("AUTO")
                && (value.get("value").and_then(Value::as_f64).is_none()
                    || !matches!(unit, Some("PERCENT" | "PIXELS")))
            {
                return Some(format!("{metric} has no representable unit/value"));
            }
        }
    }
    None
}

/// A text style's value as devup-ui reads it — the plugin's
/// `textStyleToTypography`.
///
/// `fontSize` is in pixels; `lineHeight` from a percentage is the whole-pixel
/// advance Figma draws when the size is fixed, and the percentage itself
/// (`150%`) when the size is bound to a variable or missing - CSS scales it
/// with whatever size applies -, pixels as they are, and `normal` when Figma
/// sets it automatically; `letterSpacing` is `em` from a percentage and pixels as
/// they are; the weight is read off the font style's name; and a field the
/// style binds to a variable is that variable's token. Two departures from
/// the plugin, on purpose: `Bold Italic` is `700` and italic, where the
/// plugin reads the weight off the whole style name and gets `400`; and a
/// text case is the CSS `text-transform` value — `uppercase`, not `upper`.
fn typography_value(style: &Value, variable_names: &BTreeMap<&str, String>) -> Value {
    let mut entry = Map::new();
    let font = style.get("fontName");
    let family = font
        .and_then(|font| font.get("family"))
        .and_then(Value::as_str);
    let face = font
        .and_then(|font| font.get("style"))
        .and_then(Value::as_str)
        .unwrap_or("Regular");
    if let Some(family) = family {
        entry.insert("fontFamily".to_owned(), Value::String(family.to_owned()));
    }
    if face.contains("Italic") {
        entry.insert("fontStyle".to_owned(), Value::String("italic".to_owned()));
    }
    entry.insert("fontWeight".to_owned(), Value::from(font_weight(face)));
    if let Some(size) = style.get("fontSize").and_then(Value::as_f64) {
        entry.insert("fontSize".to_owned(), Value::String(format_px(size)));
    }
    match style.get("textDecoration").and_then(Value::as_str) {
        Some("UNDERLINE") => {
            entry.insert(
                "textDecoration".to_owned(),
                Value::String("underline".to_owned()),
            );
        }
        Some("STRIKETHROUGH") => {
            entry.insert(
                "textDecoration".to_owned(),
                Value::String("line-through".to_owned()),
            );
        }
        _ => {}
    }
    let transform = match style.get("textCase").and_then(Value::as_str) {
        Some("UPPER") => Some("uppercase"),
        Some("LOWER") => Some("lowercase"),
        Some("TITLE") => Some("capitalize"),
        _ => None,
    };
    if let Some(transform) = transform {
        entry.insert(
            "textTransform".to_owned(),
            Value::String(transform.to_owned()),
        );
    }
    if let Some(line_height) = style.get("lineHeight") {
        let unit = line_height.get("unit").and_then(Value::as_str);
        let value = line_height.get("value").and_then(Value::as_f64);
        let written = match (unit, value) {
            (Some("AUTO"), _) => Some(Value::String("normal".to_owned())),
            (Some("PERCENT"), Some(percent)) => Some(Value::String(
                match style
                    .get("fontSize")
                    .and_then(Value::as_f64)
                    .filter(|_| style.pointer("/boundVariables/fontSize").is_none())
                {
                    Some(size) => format_px((size * percent / 100.0).round()),
                    None => format_percent(percent),
                },
            )),
            (Some(_), Some(pixels)) => Some(Value::String(format_px(pixels))),
            _ => None,
        };
        if let Some(written) = written {
            entry.insert("lineHeight".to_owned(), written);
        }
    }
    if let Some(spacing) = style.get("letterSpacing") {
        let unit = spacing.get("unit").and_then(Value::as_str);
        let value = spacing.get("value").and_then(Value::as_f64);
        let written = match (unit, value) {
            (Some("PERCENT"), Some(percent)) => Some(format!("{}em", percent.round() / 100.0)),
            (Some(_), Some(pixels)) => Some(format_px(pixels)),
            _ => None,
        };
        if let Some(written) = written {
            entry.insert("letterSpacing".to_owned(), Value::String(written));
        }
    }
    if let Some(bound) = style.get("boundVariables").and_then(Value::as_object) {
        for field in [
            "fontFamily",
            "fontSize",
            "fontStyle",
            "fontWeight",
            "letterSpacing",
            "lineHeight",
        ] {
            if let Some(token) = bound
                .get(field)
                .and_then(|alias| alias.get("id"))
                .and_then(Value::as_str)
                .and_then(|id| variable_names.get(id))
            {
                entry.insert(field.to_owned(), Value::String(format!("${token}")));
            }
        }
    }
    Value::Object(entry)
}

/// The weight a font style's name means — the plugin's `getFontWeight`,
/// with `Italic` set aside first so `Bold Italic` is still bold.
fn font_weight(face: &str) -> u32 {
    let name = face
        .replace("Italic", "")
        .replace([' ', '-', '_'], "")
        .to_ascii_lowercase();
    match name.as_str() {
        "thin" | "hairline" => 100,
        "extralight" | "ultralight" => 200,
        "light" => 300,
        "" | "normal" | "regular" | "book" => 400,
        "medium" => 500,
        "semibold" | "demibold" => 600,
        "bold" => 700,
        "extrabold" | "ultrabold" => 800,
        "black" | "heavy" => 900,
        other => match other.parse::<u32>() {
            Ok(number) if (1..=9).contains(&number) => number * 100,
            Ok(number) => number,
            Err(_) => 400,
        },
    }
}

/// An effect style's visible shadows as one CSS `box-shadow` — the plugin's
/// `effectStyleToCssShadow`. `None` when it casts none.
fn shadow_value(effects: &Value) -> Option<String> {
    let parts = effects
        .as_array()?
        .iter()
        .filter(|effect| effect.get("visible").and_then(Value::as_bool) != Some(false))
        .filter_map(|effect| {
            let kind = effect.get("type").and_then(Value::as_str)?;
            let inset = match kind {
                "DROP_SHADOW" => "",
                "INNER_SHADOW" => "inset ",
                _ => return None,
            };
            let offset = effect.get("offset");
            let x = offset
                .and_then(|offset| offset.get("x"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let y = offset
                .and_then(|offset| offset.get("y"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let radius = effect.get("radius").and_then(Value::as_f64).unwrap_or(0.0);
            let spread = effect.get("spread").and_then(Value::as_f64).unwrap_or(0.0);
            let color = color_value(effect.get("color")?)?;
            let length = |value: f64| {
                if value == 0.0 {
                    "0".to_owned()
                } else {
                    format_px(value)
                }
            };
            Some(format!(
                "{inset}{} {} {} {} {color}",
                length(x),
                length(y),
                length(radius),
                length(spread)
            ))
        })
        .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join(", "))
}
