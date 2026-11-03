//! Conservative SVG identities. Unsafe or unsupported constructs fall back to byte identity.
use quick_xml::{Reader, XmlVersion, events::Event};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Serialize)]
struct Element {
    tag: String,
    attrs: BTreeMap<String, String>,
    children: Vec<Element>,
}

pub(super) struct Identity {
    pub key: String,
    pub geometry: String,
    pub grayscale_filter: Option<String>,
}

// Identities use exact decimal fractions, never tolerance or rounded coordinates.
#[derive(Clone, Copy)]
struct Fraction(i128, i128);

impl Fraction {
    fn parse(value: &str) -> Option<Self> {
        let (base, exponent) = value
            .split_once(['e', 'E'])
            .map_or(Some((value, 0)), |(base, exponent)| {
                Some((base, exponent.parse::<i32>().ok()?))
            })?;
        let negative = base.starts_with('-');
        let unsigned = base.trim_start_matches(['-', '+']);
        let (whole, decimal) = unsigned.split_once('.').unwrap_or((unsigned, ""));
        if whole.is_empty() && decimal.is_empty() {
            return None;
        }
        if !whole
            .bytes()
            .chain(decimal.bytes())
            .all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let mut numerator = format!("{whole}{decimal}").parse::<i128>().ok()?;
        if negative {
            numerator = numerator.checked_neg()?;
        }
        let power = i32::try_from(decimal.len()).ok()?.checked_sub(exponent)?;
        let denominator = if power >= 0 {
            10_i128.checked_pow(power.try_into().ok()?)?
        } else {
            numerator = numerator
                .checked_mul(10_i128.checked_pow(power.checked_neg()?.try_into().ok()?)?)?;
            1
        };
        Some(Self::reduced(numerator, denominator))
    }

    fn reduced(numerator: i128, denominator: i128) -> Self {
        fn gcd(mut a: u128, mut b: u128) -> u128 {
            while b != 0 {
                (a, b) = (b, a % b);
            }
            a
        }
        let divisor = gcd(numerator.unsigned_abs(), denominator as u128) as i128;
        Self(numerator / divisor, denominator / divisor)
    }

    fn divided_by(self, other: Self) -> Option<String> {
        if other.0 <= 0 {
            return None;
        }
        let left = Self::reduced(self.0, other.0);
        let right = Self::reduced(other.1, self.1);
        let result = Self::reduced(left.0.checked_mul(right.0)?, left.1.checked_mul(right.1)?);
        Some(if result.1 == 1 {
            result.0.to_string()
        } else {
            format!("{}/{}", result.0, result.1)
        })
    }
}

fn numbers(value: &str) -> Option<Vec<Fraction>> {
    value
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(Fraction::parse)
        .collect()
}

fn number(value: f64) -> String {
    if value == 0.0 {
        "0".into()
    } else {
        format!("{value:.10}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .into()
    }
}

fn path(value: &str, width: Fraction, height: Fraction) -> Option<String> {
    let mut tokens = Vec::new();
    let mut cursor = 0;
    let bytes = value.as_bytes();
    let mut command = ' ';
    let mut slot = 0;
    while cursor < bytes.len() {
        let c = bytes[cursor] as char;
        if c.is_ascii_whitespace() || c == ',' {
            cursor += 1;
            continue;
        }
        if c.is_ascii_alphabetic() {
            if !"MmLlHhVvCcSsQqTtAaZz".contains(c) {
                return None;
            }
            command = c;
            slot = 0;
            tokens.push(c.to_string());
            cursor += 1;
            continue;
        }
        let start = cursor;
        if matches!(bytes[cursor], b'+' | b'-') {
            cursor += 1;
        }
        while cursor < bytes.len() && (bytes[cursor].is_ascii_digit() || bytes[cursor] == b'.') {
            cursor += 1;
        }
        if cursor < bytes.len() && matches!(bytes[cursor], b'e' | b'E') {
            cursor += 1;
            if cursor < bytes.len() && matches!(bytes[cursor], b'+' | b'-') {
                cursor += 1;
            }
            while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                cursor += 1;
            }
        }
        if start == cursor {
            return None;
        }
        let n = Fraction::parse(&value[start..cursor])?;
        let upper = command.to_ascii_uppercase();
        let divisor = match upper {
            'H' => width,
            'V' => height,
            'A' => match slot % 7 {
                0 | 5 => width,
                1 | 6 => height,
                _ => Fraction(1, 1),
            },
            'M' | 'L' | 'C' | 'S' | 'Q' | 'T' => {
                if slot % 2 == 0 {
                    width
                } else {
                    height
                }
            }
            _ => return None,
        };
        tokens.push(n.divided_by(divisor)?);
        slot += 1;
    }
    Some(tokens.join(" "))
}

fn gray(value: &str) -> Option<u8> {
    let rgb = match value {
        "black" => 0,
        "white" => 0xffffff,
        _ => u32::from_str_radix(value.strip_prefix('#')?.get(..6)?, 16).ok()?,
    };
    let (r, g, b) = ((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
    (r == g && g == b).then_some(r)
}

fn opaque_color(value: &str) -> bool {
    matches!(value, "black" | "white" | "red" | "green" | "blue")
        || value.strip_prefix('#').is_some_and(|hex| {
            matches!(hex.len(), 3 | 6) && hex.bytes().all(|b| b.is_ascii_hexdigit())
        })
}

pub(super) fn identity(bytes: &[u8], mask: bool) -> Option<Identity> {
    let text = std::str::from_utf8(bytes).ok()?;
    if text.len() > 4 * 1024 * 1024 {
        return None;
    }
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut stack = Vec::<Element>::new();
    let mut root = None;
    loop {
        match reader.read_event().ok()? {
            Event::Start(tag) | Event::Empty(tag) => {
                let name = tag.name().as_ref().to_owned();
                if ![
                    "svg",
                    "g",
                    "path",
                    "rect",
                    "circle",
                    "ellipse",
                    "line",
                    "polyline",
                    "polygon",
                    "defs",
                    "linearGradient",
                    "radialGradient",
                    "stop",
                    "clipPath",
                    "mask",
                ]
                .contains(&name.as_str())
                {
                    return None;
                }
                let attrs = tag
                    .attributes()
                    .map(|a| {
                        let a = a.ok()?;
                        Some((
                            a.key.as_ref().to_owned(),
                            a.normalized_value(XmlVersion::Implicit1_0)
                                .ok()?
                                .into_owned(),
                        ))
                    })
                    .collect::<Option<BTreeMap<_, _>>>()?;
                if attrs.iter().any(|(key, value)| {
                    key.starts_with("on")
                        || key == "style"
                        || ((key == "href" || key == "xlink:href") && !value.starts_with('#'))
                        || (value.contains("url(") && !value.starts_with("url(#"))
                }) {
                    return None;
                }
                let element = Element {
                    tag: name,
                    attrs,
                    children: Vec::new(),
                };
                // quick-xml's empty syntax is stable at the event byte boundary.
                if text[..reader.buffer_position() as usize]
                    .trim_end()
                    .ends_with("/>")
                {
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(element);
                    } else if root.replace(element).is_some() {
                        return None;
                    }
                } else {
                    stack.push(element);
                }
            }
            Event::End(_) => {
                let element = stack.pop()?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(element);
                } else if root.replace(element).is_some() {
                    return None;
                }
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) => {}
            _ => return None,
        }
    }
    if !stack.is_empty() {
        return None;
    }
    let mut root = root?;
    if root.tag != "svg" {
        return None;
    }
    let view = numbers(root.attrs.get("viewBox")?)?;
    if view.len() != 4 || view[0].0 != 0 || view[1].0 != 0 || view[2].0 <= 0 || view[3].0 <= 0 {
        return None;
    }
    fn ids(element: &Element, out: &mut BTreeMap<String, String>) -> Option<()> {
        if let Some(id) = element.attrs.get("id") {
            let next = format!("id{}", out.len());
            if out.insert(id.clone(), next).is_some() {
                return None;
            }
        }
        for child in &element.children {
            ids(child, out)?;
        }
        Some(())
    }
    let mut id_map = BTreeMap::new();
    ids(&root, &mut id_map)?;
    fn complex(e: &Element) -> bool {
        matches!(e.tag.as_str(), "defs" | "mask" | "clipPath") || e.children.iter().any(complex)
    }
    let mut flat = !complex(&root);
    fn scalable(e: &Element) -> bool {
        !e.attrs.contains_key("transform")
            && !e.attrs.contains_key("vector-effect")
            && !e.children.iter().any(|child| child.tag == "svg")
            && e.attrs.iter().all(|(key, value)| {
                ![
                    "x",
                    "x1",
                    "x2",
                    "cx",
                    "rx",
                    "width",
                    "r",
                    "stroke-width",
                    "y",
                    "y1",
                    "y2",
                    "cy",
                    "ry",
                    "height",
                    "stroke-dashoffset",
                ]
                .contains(&key.as_str())
                    || Fraction::parse(value).is_some()
                    || value.ends_with('%')
            })
            && e.children.iter().all(scalable)
    }
    let scale = !complex(&root) && scalable(&root);
    let mut paint_values = Vec::new();
    fn effective_paint(e: &Element, fill: &str, stroke: &str, out: &mut Vec<String>) {
        let fill = e.attrs.get("fill").map_or(fill, String::as_str);
        let stroke = e.attrs.get("stroke").map_or(stroke, String::as_str);
        if matches!(
            e.tag.as_str(),
            "path" | "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon"
        ) {
            if e.tag != "line" && fill != "none" {
                out.push(fill.into());
            }
            if stroke != "none" {
                out.push(stroke.into());
            }
        }
        for child in &e.children {
            effective_paint(child, fill, stroke, out);
        }
    }
    effective_paint(&root, "black", "none", &mut paint_values);
    fn normalize(
        e: &mut Element,
        ids: &BTreeMap<String, String>,
        dims: (Fraction, Fraction),
        scale: bool,
        paint: &mut Vec<String>,
        flat: &mut bool,
    ) -> Option<()> {
        for (key, value) in &mut e.attrs {
            if key == "id" {
                *value = ids.get(value)?.clone();
                continue;
            }
            if let Some(id) = value
                .strip_prefix("url(#")
                .and_then(|s| s.strip_suffix(')'))
            {
                *value = format!("url(#{})", ids.get(id)?);
            } else if matches!(key.as_str(), "href" | "xlink:href") {
                *value = format!("#{}", ids.get(value.strip_prefix('#')?)?);
            } else if value.contains("url(") {
                return None;
            }
            if ["fill", "stroke", "stop-color"].contains(&key.as_str()) && value != "none" {
                if !opaque_color(value) {
                    *flat = false;
                } else {
                    paint.push(value.clone());
                }
            }
            if key == "transform" {
                *flat = false;
            }
            if scale {
                if key == "d" {
                    *value = path(value, dims.0, dims.1)?;
                } else if key == "points" {
                    let points = numbers(value)?;
                    if points.len() % 2 != 0 {
                        return None;
                    }
                    *value = points
                        .iter()
                        .enumerate()
                        .map(|(i, n)| n.divided_by(if i % 2 == 0 { dims.0 } else { dims.1 }))
                        .collect::<Option<Vec<_>>>()?
                        .join(" ");
                } else if key == "stroke-dasharray" && value != "none" {
                    *value = numbers(value)?
                        .iter()
                        .map(|n| n.divided_by(dims.0))
                        .collect::<Option<Vec<_>>>()?
                        .join(" ");
                } else if [
                    "x",
                    "x1",
                    "x2",
                    "cx",
                    "rx",
                    "width",
                    "r",
                    "stroke-width",
                    "stroke-dashoffset",
                ]
                .contains(&key.as_str())
                {
                    if let Some(n) = Fraction::parse(value) {
                        *value = n.divided_by(dims.0)?;
                    }
                } else if ["y", "y1", "y2", "cy", "ry", "height"].contains(&key.as_str())
                    && let Some(n) = Fraction::parse(value) {
                    *value = n.divided_by(dims.1)?;
                }
            }
        }
        for child in &mut e.children {
            normalize(child, ids, dims, scale, paint, flat)?;
        }
        Some(())
    }
    normalize(
        &mut root,
        &id_map,
        (view[2], view[3]),
        scale,
        &mut paint_values,
        &mut flat,
    )?;
    root.attrs.remove("width");
    root.attrs.remove("height");
    if scale {
        root.attrs.insert(
            "viewBox".into(),
            format!("0 0 {} 1", view[2].divided_by(view[3])?),
        );
    }
    let full = serde_json::to_vec(&root).ok()?;
    fn strip(e: &mut Element) {
        for key in ["fill", "stroke", "stop-color"] {
            if let Some(value) = e.attrs.get_mut(key)
                && value != "none"
            {
                *value = "paint".into();
            }
        }
        for child in &mut e.children {
            strip(child);
        }
    }
    let grayscale = if flat {
        paint_values
            .first()
            .and_then(|v| gray(v))
            .filter(|g| paint_values.iter().all(|v| gray(v) == Some(*g)))
    } else {
        None
    };
    if flat {
        strip(&mut root);
    }
    let geometry = serde_json::to_vec(&root).ok()?;
    let key = if mask && flat { &geometry } else { &full };
    Some(Identity {
        key: super::sha256_hex(key),
        geometry: super::sha256_hex(&geometry),
        grayscale_filter: grayscale.map(|g| {
            format!(
                "grayscale(1) brightness(0) invert({})",
                number(f64::from(g) / 255.0)
            )
        }),
    })
}
