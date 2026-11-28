/// A compared property value: a length or number, or a colour/family string.
#[derive(Debug, Clone, PartialEq)]
enum Prop {
    Num(f64),
    Text(String),
}
impl Prop {
    fn to_json(&self) -> Value {
        match self {
            Self::Num(n) => json!((n * 1000.0).round() / 1000.0),
            Self::Text(s) => json!(s),
        }
    }
}

/// Property name -> the category the requirement names it under.
const PROPERTIES: &[(&str, &str)] = &[
    ("width", "size"),
    ("height", "size"),
    ("x", "position"),
    ("y", "position"),
    ("color", "color"),
    ("backgroundColor", "color"),
    ("borderColor", "border"),
    ("borderWidth", "border"),
    ("borderRadius", "border"),
    ("fontFamily", "typography"),
    ("fontSize", "typography"),
    ("fontWeight", "typography"),
    ("lineHeight", "typography"),
    ("letterSpacing", "typography"),
];

/// Snapshot fields that carry each property; a field error there is a gap.
const FIELD_PROPERTIES: &[(&str, &[&str])] = &[
    ("width", &["width"]),
    ("height", &["height"]),
    ("absoluteBoundingBox", &["x", "y"]),
    ("fills", &["color", "backgroundColor"]),
    ("strokes", &["borderColor", "borderWidth"]),
    ("strokeWeight", &["borderWidth"]),
    ("cornerRadius", &["borderRadius"]),
    ("fontName", &["fontFamily", "fontWeight"]),
    ("fontSize", &["fontSize"]),
    ("fontWeight", &["fontWeight"]),
    ("lineHeight", &["lineHeight"]),
    ("letterSpacing", &["letterSpacing"]),
    (
        "styledTextSegments",
        &[
            "fontFamily",
            "fontSize",
            "fontWeight",
            "lineHeight",
            "letterSpacing",
            "color",
        ],
    ),
];

fn category(property: &str) -> &'static str {
    PROPERTIES
        .iter()
        .find(|(name, _)| *name == property)
        .map(|(_, c)| *c)
        .unwrap_or("other")
}

/// The compared properties an unreadable snapshot field would have produced.
fn field_properties(field: &str) -> &'static [&'static str] {
    FIELD_PROPERTIES
        .iter()
        .find(|(name, _)| *name == field)
        .map(|(_, p)| *p)
        .unwrap_or(&[])
}

fn snapshot_props(style: &StyleSnapshot) -> BTreeMap<&'static str, Prop> {
    let mut props = BTreeMap::new();
    let numbers = [
        ("x", style.x),
        ("y", style.y),
        ("width", style.width),
        ("height", style.height),
        ("borderWidth", style.border_width),
        ("borderRadius", style.border_radius),
        ("fontSize", style.font_size),
        ("fontWeight", style.font_weight),
        ("lineHeight", style.line_height),
        ("letterSpacing", style.letter_spacing),
    ];
    for (name, value) in numbers {
        if let Some(value) = value.filter(|v| v.is_finite()) {
            props.insert(name, Prop::Num(value));
        }
    }
    let texts = [
        ("color", &style.color),
        ("backgroundColor", &style.background_color),
        ("borderColor", &style.border_color),
        ("fontFamily", &style.font_family),
    ];
    for (name, value) in texts {
        if let Some(value) = present(value) {
            props.insert(name, Prop::Text(value.to_owned()));
        }
    }
    props
}

/// The first visible solid paint as #rrggbbaa.
fn solid_paint(paints: Option<&Value>) -> Option<String> {
    let paint = array(paints?).iter().find(|p| p["visible"] != false)?;
    if paint["type"] != "SOLID" {
        return None;
    }
    let channel = |key: &str| paint["color"][key].as_f64();
    let alpha =
        paint["color"]["a"].as_f64().unwrap_or(1.0) * paint["opacity"].as_f64().unwrap_or(1.0);
    let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some(format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        byte(channel("r")?),
        byte(channel("g")?),
        byte(channel("b")?),
        byte(alpha)
    ))
}

/// A Figma length that is `12`, `{unit:"PIXELS",value:12}` or a percent of the font size.
fn figma_length(value: Option<&Value>, font_size: Option<f64>) -> Option<f64> {
    let value = value?;
    if let Some(n) = value.as_f64() {
        return Some(n);
    }
    let amount = value["value"].as_f64()?;
    match text(&value["unit"]) {
        "PIXELS" => Some(amount),
        "PERCENT" => font_size.map(|size| size * amount / 100.0),
        _ => None,
    }
}

fn font_weight_from_style(style: &str) -> Option<f64> {
    let style = style.to_ascii_lowercase().replace([' ', '-'], "");
    [
        ("thin", 100.0),
        ("extralight", 200.0),
        ("ultralight", 200.0),
        ("light", 300.0),
        ("regular", 400.0),
        ("normal", 400.0),
        ("medium", 500.0),
        ("semibold", 600.0),
        ("demibold", 600.0),
        ("extrabold", 800.0),
        ("ultrabold", 800.0),
        ("bold", 700.0),
        ("black", 900.0),
        ("heavy", 900.0),
    ]
    .iter()
    .find(|(name, _)| style.starts_with(name))
    .map(|(_, weight)| *weight)
}

/// The design values of one node, read from the cached snapshot. Unreadable
/// fields and missing nodes are reported by `collect_gaps`.
fn design_from_snapshot(payload: &CollectedPayload, node_id: &str) -> BTreeMap<&'static str, Prop> {
    let mut props = BTreeMap::new();
    let Some(node) = payload.snapshot.nodes.get(node_id) else {
        return props;
    };
    let view = node.typed_view();
    if let Some(width) = view.number("width") {
        props.insert("width", Prop::Num(width));
    }
    if let Some(height) = view.number("height") {
        props.insert("height", Prop::Num(height));
    }
    let origin = payload
        .target
        .node_id
        .as_deref()
        .filter(|origin| *origin != node_id)
        .and_then(|origin| payload.snapshot.nodes.get(origin))
        .and_then(|n| n.typed_view().value("absoluteBoundingBox").cloned());
    if let (Some(origin), Some(own)) = (origin, view.value("absoluteBoundingBox"))
        && let (Some(ox), Some(oy), Some(x), Some(y)) = (
            origin["x"].as_f64(),
            origin["y"].as_f64(),
            own["x"].as_f64(),
            own["y"].as_f64(),
        )
    {
        props.insert("x", Prop::Num(x - ox));
        props.insert("y", Prop::Num(y - oy));
    }
    let is_text = node.node_type == "TEXT";
    let segment = view
        .value("styledTextSegments")
        .and_then(|s| s.as_array())
        .and_then(|s| s.first())
        .cloned()
        .unwrap_or(Value::Null);
    let field = |name: &str| {
        view.value(name)
            .filter(|v| !v.is_null() && v.get("$error").is_none())
            .or_else(|| segment.get(name).filter(|v| !v.is_null()))
    };
    if let Some(color) = solid_paint(field("fills")) {
        props.insert(
            if is_text { "color" } else { "backgroundColor" },
            Prop::Text(color),
        );
    }
    if let Some(color) = solid_paint(view.value("strokes")) {
        props.insert("borderColor", Prop::Text(color));
        if let Some(weight) = view.number("strokeWeight") {
            props.insert("borderWidth", Prop::Num(weight));
        }
    }
    if let Some(radius) = view.number("cornerRadius") {
        props.insert("borderRadius", Prop::Num(radius));
    }
    if is_text {
        let font_size = field("fontSize").and_then(Value::as_f64);
        if let Some(size) = font_size {
            props.insert("fontSize", Prop::Num(size));
        }
        if let Some(name) = field("fontName") {
            if let Some(family) = name["family"].as_str() {
                props.insert("fontFamily", Prop::Text(family.to_owned()));
            }
            if let Some(weight) = name["style"].as_str().and_then(font_weight_from_style) {
                props.insert("fontWeight", Prop::Num(weight));
            }
        }
        if let Some(weight) = field("fontWeight").and_then(Value::as_f64) {
            props.insert("fontWeight", Prop::Num(weight));
        }
        if let Some(line) = figma_length(field("lineHeight"), font_size) {
            props.insert("lineHeight", Prop::Num(line));
        }
        if let Some(spacing) = figma_length(field("letterSpacing"), font_size) {
            props.insert("letterSpacing", Prop::Num(spacing));
        }
    }
    props
}

/// RGBA bytes from the colour spellings CSS computed style and Figma use.
fn parse_color(value: &str) -> Option<[u8; 4]> {
    let value = value.trim().to_ascii_lowercase();
    if value == "transparent" {
        return Some([0, 0, 0, 0]);
    }
    if let Some(hex) = value.strip_prefix('#') {
        let digits = match hex.len() {
            3 | 4 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
            6 | 8 => hex.to_owned(),
            _ => return None,
        };
        let byte = |i: usize| u8::from_str_radix(digits.get(i..i + 2)?, 16).ok();
        return Some([
            byte(0)?,
            byte(2)?,
            byte(4)?,
            if digits.len() == 8 { byte(6)? } else { 255 },
        ]);
    }
    let inner = value
        .strip_prefix("rgba(")
        .or_else(|| value.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let parts = inner
        .split([',', ' ', '/'])
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>();
    if !(3..=4).contains(&parts.len()) {
        return None;
    }
    let channel = |p: &str| -> Option<u8> {
        if let Some(percent) = p.strip_suffix('%') {
            return Some(
                (percent.parse::<f64>().ok()? / 100.0 * 255.0)
                    .round()
                    .clamp(0.0, 255.0) as u8,
            );
        }
        Some(p.parse::<f64>().ok()?.round().clamp(0.0, 255.0) as u8)
    };
    let alpha = match parts.get(3) {
        None => 255,
        Some(p) => {
            if let Some(percent) = p.strip_suffix('%') {
                (percent.parse::<f64>().ok()? / 100.0 * 255.0)
                    .round()
                    .clamp(0.0, 255.0) as u8
            } else {
                (p.parse::<f64>().ok()? * 255.0).round().clamp(0.0, 255.0) as u8
            }
        }
    };
    Some([
        channel(parts[0])?,
        channel(parts[1])?,
        channel(parts[2])?,
        alpha,
    ])
}

fn first_family(value: &str) -> String {
    value
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches(['"', '\''])
        .to_ascii_lowercase()
}

/// Compare every design property with the implementation. A property the
/// implementation did not measure is `not-measured`, never a match.
fn compare_props(
    design: &BTreeMap<&'static str, Prop>,
    implementation: &BTreeMap<&'static str, Prop>,
    tolerance: f64,
) -> Vec<Value> {
    design
        .iter()
        .map(|(name, expected)| {
            let mut row =
                json!({"property":name,"category":category(name),"design":expected.to_json()});
            let Some(actual) = implementation.get(name) else {
                row["status"] = json!("not-measured");
                return row;
            };
            row["implementation"] = actual.to_json();
            let (matched, detail) = match (expected, actual) {
                (Prop::Num(a), Prop::Num(b)) => {
                    let limit = if *name == "fontWeight" {
                        0.0
                    } else {
                        tolerance
                    };
                    let delta = b - a;
                    row["delta"] = json!((delta * 1000.0).round() / 1000.0);
                    (
                        delta.abs() <= limit + f64::EPSILON,
                        format!("tolerance {limit}px"),
                    )
                }
                (Prop::Text(a), Prop::Text(b)) if *name == "fontFamily" => (
                    first_family(a) == first_family(b),
                    "first family, case-insensitive".into(),
                ),
                (Prop::Text(a), Prop::Text(b)) => match (parse_color(a), parse_color(b)) {
                    (Some(x), Some(y)) => {
                        let max = x
                            .iter()
                            .zip(y)
                            .map(|(p, q)| p.abs_diff(q))
                            .max()
                            .unwrap_or(0);
                        row["maxChannelDelta"] = json!(max);
                        (max <= 1, "RGBA channel delta up to 1".into())
                    }
                    _ => (
                        a.trim().eq_ignore_ascii_case(b.trim()),
                        "unparsed colour, literal comparison".into(),
                    ),
                },
                _ => (false, "value types differ".into()),
            };
            row["status"] = json!(if matched { "match" } else { "mismatch" });
            row["rule"] = json!(detail);
            row
        })
        .collect()
}
