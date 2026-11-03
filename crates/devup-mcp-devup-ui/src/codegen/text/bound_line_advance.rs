fn source_line_height(
    value: Option<&Value>,
    size: Option<&Value>,
    bound: Option<&Value>,
) -> Option<String> {
    if bound.and_then(|b| b.get("fontSize")).is_some()
        && bound.and_then(|b| b.get("lineHeight")).is_none()
        && size
            .and_then(Value::as_f64)
            .is_some_and(|size| size.is_finite() && size > 0.0)
        && value?.get("unit").and_then(Value::as_str) == Some("PERCENT")
        && let Some(percent) = value?
            .get("value")
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && *v >= 0.0)
    {
        return Some(format!(
            "round(nearest, {}em, 1px)",
            (percent * 100.0).round() / 10_000.0
        ));
    }
    line_height(value, line_height_font_size(size, bound))
}
