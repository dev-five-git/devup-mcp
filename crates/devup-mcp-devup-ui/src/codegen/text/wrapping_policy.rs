fn contains_korean(text: &str) -> bool {
    text.chars().any(|character| matches!(character as u32, 0xAC00..=0xD7AF | 0x1100..=0x11FF | 0x3130..=0x318F))
}

/// Detect Korean from captured plain text and styled segments.
fn segments_contain_korean(view: &TypedNode<'_>) -> bool {
    view.string("characters").is_some_and(contains_korean) || view.value("styledTextSegments")
        .and_then(Value::as_array)
        .is_some_and(|segments| {
            segments.iter().any(|segment| {
                segment
                    .get("characters")
                    .and_then(Value::as_str)
                    .is_some_and(|text| {
                        contains_korean(text)
                    })
            })
        })
}

fn theme_has_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Array(values) => values.iter().any(|value| theme_has_key(value, key)),
        Value::Object(value) => value.contains_key(key),
        _ => false,
    }
}

/// Korean copy wraps at word boundaries even in measured fixed-width text.
/// `textWrap="pretty"` lets the browser rebalance word-boundary breaks so
/// keep-all does not leave a large trailing gap or a single orphaned word.
/// `text-wrap` is a CSS shorthand that also resets `text-wrap-mode` to
/// `wrap`, so it is never added to text whose `whiteSpace` (`pre`/`nowrap`)
/// deliberately disables soft wrapping: it would re-enable wrapping there.
fn push_korean_wrap_policy(view: &TypedNode<'_>, theme: Option<&Value>, props: &mut Vec<Prop>) {
    if !segments_contain_korean(view)
        || props.iter().any(|(name, _)| name == "wordBreak")
        || theme.is_some_and(|value| theme_has_key(value, "wordBreak"))
    {
        return;
    }
    string_prop(props, "wordBreak", "keep-all");
    if !props.iter().any(|(name, _)| name == "overflowWrap") {
        string_prop(props, "overflowWrap", "break-word");
    }
    if !props.iter().any(|(name, _)| name == "textWrap")
        && !props.iter().any(|(name, value)| {
            name == "whiteSpace"
                && matches!(value, PropValue::String(mode) if mode == "pre" || mode == "nowrap")
        })
        && !theme.is_some_and(|value| theme_has_key(value, "textWrap"))
    {
        string_prop(props, "textWrap", "pretty");
    }
}

