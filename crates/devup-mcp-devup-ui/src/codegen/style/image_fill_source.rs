

/// The file an image fill refers to.
///
/// Every image fill once resolved to a single hard-coded `/icons/image.png`,
/// which lost three separate things: a raster was pointed at the icon folder,
/// unrelated images from different nodes all claimed the same file and so
/// overwrote one another on disk, and two fills on one node produced the
/// identical URL twice over. The manifest identifies a fill as
/// `{nodeId}:fills:{index}`, so the reference keeps the node's name and, past
/// the first fill, its index — a lone fill keeps the plain
/// `/images/{name}.png` the `<Image>` element already emits, so the two agree
/// on the same asset.
fn image_fill_source(
    snapshot: &Snapshot,
    node: &RawNode,
    fill_index: usize,
    per_node: bool,
) -> String {
    let source = if fill_index == 0 {
        asset_source(snapshot, node, "images", "png", per_node)
    } else {
        let stem = asset_stem(snapshot, node, per_node);
        format!("/images/{stem}-{fill_index}.png")
    };
    if source.contains(' ') {
        format!("'{source}'")
    } else {
        source
    }
}

/// The file an asset node is drawn from: `/{folder}/{stem}.{extension}`.
///
/// The plugin draws an instance from its main component and names the file
/// after that node, so every instance of a variant is `Property 1=search.svg`:
/// one file for the icon wherever it is used, but the same file for every
/// component set that has a `search` variant, each overwriting the last. The
/// layer name a designer gave the node is used instead, and where two assets
/// in the snapshot that are not the same thing would share it, each says what
/// it is: see `asset_stem`.
pub(crate) fn asset_source(
    snapshot: &Snapshot,
    node: &RawNode,
    folder: &str,
    extension: &str,
    per_node: bool,
) -> String {
    format!(
        "/{folder}/{}.{extension}",
        asset_stem(snapshot, node, per_node)
    )
}

/// The path the generated code refers to for an asset node - `/icons/x.svg`
/// for a vector, `/images/x.png` for the first image fill - so a manifest
/// can say where the code expects each asset. `None` for a node the code
/// does not draw from a file.
pub fn asset_path(snapshot: &Snapshot, node_id: &str, per_node: bool) -> Option<String> {
    let node = snapshot.nodes.get(node_id)?;
    let kind = asset_kind(snapshot, node)?;
    let (folder, extension) = match kind {
        AssetKind::Svg | AssetKind::SvgMask => ("icons", "svg"),
        _ => ("images", "png"),
    };
    Some(asset_source(snapshot, node, folder, extension, per_node))
}

/// Where the code draws one of a node's image fills from: `/images/x.png`
/// for the first fill and `/images/x-2.png` past it, the same name
/// `image_fill_source` writes into the code but without the quoting a CSS
/// `url()` puts around a name with a space in it.
///
/// This answers for any node that carries the fill, where `asset_path` only
/// answers for a node the code draws entirely from a file. A section painted
/// over a photograph is a layout box holding children, not an asset - but the
/// photograph on it is still a file the code points at, and a caller has to
/// be told where.
pub fn image_fill_path(
    snapshot: &Snapshot,
    node_id: &str,
    fill_index: usize,
    per_node: bool,
) -> Option<String> {
    let node = snapshot.nodes.get(node_id)?;
    let stem = asset_stem(snapshot, node, per_node);
    Some(if fill_index == 0 {
        format!("/images/{stem}.png")
    } else {
        format!("/images/{stem}-{fill_index}.png")
    })
}

/// The file name an asset node gets, without folder or extension: readable
/// kebab-case; distinct things sharing a name get -2, -3 in node-id order.
pub(crate) fn asset_stem(snapshot: &Snapshot, node: &RawNode, per_node: bool) -> String {
    let canonical = image_hash(node).and_then(|hash| {
        snapshot.nodes.values().filter(|other| image_hash(other) == Some(hash)).min_by_key(|other| &other.id)
    }).unwrap_or(node);
    let base = readable_stem(snapshot, canonical);
    let own = asset_group_key(snapshot, node, per_node);
    let mut firsts = std::collections::BTreeMap::<String, &str>::new();
    for other in snapshot.nodes.values() {
        let is_asset = asset_kind(snapshot, other).is_some() || image_hash(other).is_some();
        let same_image = image_hash(node).is_some_and(|hash| image_hash(other) == Some(hash));
        if other.id != node.id && (!is_asset || (!same_image && readable_stem(snapshot, other) != base)) {
            continue;
        }
        let first = firsts
            .entry(asset_group_key(snapshot, other, per_node))
            .or_insert(other.id.as_str());
        if other.id.as_str() < *first {
            *first = other.id.as_str();
        }
    }
    let mut ids = firsts.values().copied().collect::<Vec<_>>();
    ids.sort_unstable();
    let own_first = firsts.get(&own).copied().unwrap_or(node.id.as_str());
    match ids.iter().position(|id| *id == own_first) {
        Some(index) if index > 0 => format!("{base}-{}", index + 1),
        _ => base,
    }
}

/// Lowercase `[a-z0-9-]` only, hyphens collapsed, at most 48 characters.
fn kebab_name(name: &str) -> String {
    let mut slug = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.truncate(48);
    slug.trim_end_matches('-').to_owned()
}

fn generic_role(role: &str) -> bool {
    let word = role.trim_end_matches(|c: char| c.is_ascii_digit() || c == '-');
    word.is_empty()
        || matches!(
            word,
            "rectangle" | "image" | "frame" | "vector" | "group" | "ellipse" | "picture" | "layer"
        )
}

/// The layer name as a role, prefixed by the nearest meaningful ancestor when
/// the layer name is generic, and by its variant when it is a variant instance.
fn readable_stem(snapshot: &Snapshot, node: &RawNode) -> String {
    let role = kebab_name(&layer_name(node));
    if !generic_role(&role) {
        return role;
    }
    let mut current = node.typed_view().string("parentId").map(str::to_owned);
    let mut hops = 0;
    while let Some(id) = current {
        hops += 1;
        let Some(parent) = snapshot.nodes.get(&id).filter(|_| hops <= 32) else {
            break;
        };
        let section = kebab_name(&layer_name(parent));
        if !generic_role(&section) {
            let word = if role.is_empty() { "image" } else { &role };
            return format!("{section}-{word}");
        }
        current = parent.typed_view().string("parentId").map(str::to_owned);
    }
    if role.is_empty() { "asset".to_owned() } else { role }
}

fn image_hash(node: &RawNode) -> Option<&str> {
    fills(node)?
        .iter()
        .find(|p| p["type"] == "IMAGE" && p["visible"].as_bool() != Some(false))?["imageHash"]
        .as_str()
}

/// What makes two assets the same file: identical image bytes, else the same
/// variant; raster instances (which may override image/crop) and per-node
/// naming stay one file per node.
fn asset_group_key(snapshot: &Snapshot, node: &RawNode, per_node: bool) -> String {
    if let Some(hash) = image_hash(node) {
        return format!("hash:{hash}");
    }
    let raster_instance = node.typed_view().node_type() == "INSTANCE"
        && asset_kind(snapshot, node) == Some(AssetKind::Png);
    if per_node || raster_instance {
        return format!("node:{}", node.id);
    }
    if let Some(properties) = node.typed_view().value("variantProperties").and_then(Value::as_object) {
        return format!("variant:{}", serde_json::to_string(properties).unwrap_or_default());
    }
    "plain".to_owned()
}

/// The layer name with HTML entities decoded: Figma text may arrive as
/// `R&amp;D`, which must not leak `amp` into a file name.
fn layer_name(node: &RawNode) -> String {
    node.typed_view()
        .name()
        .unwrap_or("Asset")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
        .replace(['(', ')'], "")
}

pub(super) fn paint_css(
    snapshot: &Snapshot,
    node: &RawNode,
    paint: &Value,
    fill_index: usize,
    last: bool,
    used_tokens: &mut BTreeSet<String>,
    style: StyleOptions<'_>,
) -> Option<String> {
    let StyleOptions {
        variable_tokens,
        asset_names_per_node: per_node,
        ..
    } = style;
    let kind = paint.get("type")?.as_str()?;
    match kind {
        "SOLID" => {
            let color = bound_paint_token(paint, variable_tokens)
                .map(|token| {
                    used_tokens.insert(token.clone());
                    format!("${token}")
                })
                .or_else(|| color_from_paint(paint))?;
            Some(if last {
                color
            } else {
                format!("linear-gradient({color}, {color})")
            })
        }
        "GRADIENT_LINEAR" => gradient_css(node, paint, "linear", variable_tokens),
        "GRADIENT_RADIAL" => gradient_css(node, paint, "radial", variable_tokens),
        "GRADIENT_ANGULAR" => gradient_css(node, paint, "angular", variable_tokens),
        "GRADIENT_DIAMOND" => gradient_css(node, paint, "diamond", variable_tokens),
        "IMAGE" => {
            let mapping = super::image::map_for(
                snapshot,
                node,
                Some(fill_index),
                style.asset_policies,
                per_node,
            );
            let source = mapping["assetPath"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| image_fill_source(snapshot, node, fill_index, per_node));
            let fit = mapping["css"]["background"]
                .as_str()
                .or_else(|| (paint["scaleMode"] == "TILE").then_some("repeat"))?;
            Some(format!("url({source}) {fit}"))
        }
        "PATTERN" => {
            let source_id = paint.get("sourceNodeId").and_then(Value::as_str)?;
            let source = snapshot.nodes.get(source_id);
            let name = source.map_or_else(|| "pattern".to_owned(), |node| asset_stem(snapshot, node, per_node));
            // A raster belongs with the images and a vector with the icons,
            // which is the split every other asset reference follows. This one
            // sent a png to the icon folder.
            let raster = source
                .and_then(|node| asset_kind(snapshot, node))
                .is_some_and(|kind| kind == AssetKind::Png);
            let (folder, extension) = if raster {
                ("images", "png")
            } else {
                ("icons", "svg")
            };
            let spacing = paint.get("spacing").and_then(Value::as_object);
            let x = spacing
                .and_then(|value| value.get("x"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let y = spacing
                .and_then(|value| value.get("y"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let horizontal = position_part(
                paint
                    .get("horizontalAlignment")
                    .and_then(Value::as_str)
                    .unwrap_or("START"),
                x,
                ["left", "center", "right"],
            );
            let vertical = position_part(
                paint
                    .get("verticalAlignment")
                    .and_then(Value::as_str)
                    .unwrap_or("START"),
                y,
                ["top", "center", "bottom"],
            );
            let position = [horizontal, vertical]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            Some(format!(
                "url(/{folder}/{name}.{extension}){} repeat",
                if position.is_empty() {
                    String::new()
                } else {
                    format!(" {position}")
                }
            ))
        }
        _ => None,
    }
}

fn position_part(alignment: &str, spacing: f64, values: [&str; 3]) -> Option<String> {
    if alignment == "START" && spacing == 0.0 {
        return None;
    }
    let value = match alignment {
        "CENTER" => values[1],
        "END" => values[2],
        _ => values[0],
    };
    Some(format!("{value} {}%", format_number(spacing * 100.0)))
}

fn gradient_css(
    node: &RawNode,
    paint: &Value,
    kind: &str,
    variable_tokens: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    let opacity = paint.get("opacity").and_then(Value::as_f64).unwrap_or(1.0);
    let raw_stops = paint
        .get("gradientStops")?
        .as_array()?
        .iter()
        .filter_map(|stop| {
            let mut color = stop.get("color")?.clone();
            let alpha = color.get("a").and_then(Value::as_f64).unwrap_or(1.0) * opacity;
            color
                .as_object_mut()?
                .insert("a".to_owned(), Value::from(alpha));
            // A stop bound to a variable is the token — and where the stop or
            // the paint is translucent, the token mixed with transparent by
            // that much, as the plugin's `processGradientStopColor` writes it:
            // a token names an opaque colour, and the alpha would be lost with
            // it. The report section's backdrop is a 50% gradient between two
            // tokens, `color-mix(in srgb, $primaryBg, transparent 50%)`.
            let color = bound_paint_token(stop, variable_tokens)
                .map(|token| {
                    if alpha < 1.0 {
                        format!(
                            "color-mix(in srgb, ${token}, transparent {}%)",
                            format_number((1.0 - alpha) * 100.0)
                        )
                    } else {
                        format!("${token}")
                    }
                })
                .or_else(|| color_from(&color))?;
            Some((stop.get("position")?.as_f64()?, color))
        })
        .collect::<Vec<_>>();
    let transform = paint
        .get("gradientTransform")
        .and_then(parse_transform)
        .unwrap_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
    let view = node.typed_view();
    let width = view.number("width").unwrap_or(1.0);
    let height = view.number("height").unwrap_or(1.0);
    let (prefix, positions) = match kind {
        "linear" => {
            let (angle, positions) = linear_geometry(transform, width, height, &raw_stops)?;
            (
                format!("linear-gradient({}deg, ", format_number(angle)),
                positions,
            )
        }
        "radial" => {
            let inverse = inverse_transform(transform)?;
            let center = apply_transform(inverse, [0.5, 0.5]);
            let radius = apply_transform(inverse, [1.0, 1.0]);
            (
                format!(
                    "radial-gradient({}% {}% at {}% {}%, ",
                    format_number((radius[0] - center[0]).abs() * 100.0),
                    format_number((radius[1] - center[1]).abs() * 100.0),
                    format_number(center[0] * 100.0),
                    format_number(center[1] * 100.0)
                ),
                raw_stops
                    .iter()
                    .map(|(position, _)| position * 100.0)
                    .collect(),
            )
        }
        "angular" => {
            let inverse = inverse_transform(transform)?;
            let center = apply_transform(inverse, [0.5, 0.5]);
            let start = apply_transform(inverse, [1.0, 0.5]);
            let mut angle = ((start[1] - center[1]) * height)
                .atan2((start[0] - center[0]) * width)
                .to_degrees()
                + 90.0;
            angle %= 360.0;
            if angle < 0.0 {
                angle += 360.0;
            }
            (
                format!(
                    "conic-gradient(from {}deg at {}% {}%, ",
                    format_number(angle),
                    format_number(center[0] * 100.0),
                    format_number(center[1] * 100.0)
                ),
                raw_stops
                    .iter()
                    .map(|(position, _)| position * 100.0)
                    .collect(),
            )
        }
        "diamond" => (
            String::new(),
            raw_stops
                .iter()
                .map(|(position, _)| position * 50.0)
                .collect(),
        ),
        _ => return None,
    };
    let stops = raw_stops
        .iter()
        .zip(positions)
        .map(|((_, color), position)| format!("{color} {}%", format_number(position)))
        .collect::<Vec<_>>()
        .join(", ");
    Some(match kind {
        "linear" | "radial" | "angular" => format!("{prefix}{stops})"),
        // A diamond is four linear gradients, one into each corner. The pairs
        // are written as pairs rather than as `"corner|direction"` strings
        // split back apart at runtime. Nothing outside this array ever reached
        // that split — the design comes in through `raw_stops`, `transform`
        // and `kind`, all of which are already `Option`-handled above — so the
        // only way it could have failed was a typo in the four lines below,
        // and it would have failed by killing the server rather than by
        // drawing a wrong gradient. Said as tuples, the same four facts cannot
        // be written malformed at all.
        "diamond" => [
            ("bottom right", "to bottom right"),
            ("bottom left", "to bottom left"),
            ("top left", "to top left"),
            ("top right", "to top right"),
        ]
        .map(|(position, direction)| {
            format!("linear-gradient({direction}, {stops}) {position} / 50.1% 50.1% no-repeat")
        })
        .join(", "),
        _ => return None,
    })
}

fn parse_transform(value: &Value) -> Option<[[f64; 3]; 2]> {
    let rows = value.as_array()?;
    let row = |index: usize| -> Option<[f64; 3]> {
        let values = rows.get(index)?.as_array()?;
        Some([
            values.first()?.as_f64()?,
            values.get(1)?.as_f64()?,
            values.get(2)?.as_f64()?,
        ])
    };
    Some([row(0)?, row(1)?])
}

fn inverse_transform(matrix: [[f64; 3]; 2]) -> Option<[[f64; 3]; 2]> {
    let [[a, b, c], [d, e, f]] = matrix;
    let determinant = a * e - b * d;
    (determinant.abs() > f64::EPSILON).then_some([
        [
            e / determinant,
            -b / determinant,
            (b * f - c * e) / determinant,
        ],
        [
            -d / determinant,
            a / determinant,
            (c * d - a * f) / determinant,
        ],
    ])
}

fn apply_transform(matrix: [[f64; 3]; 2], point: [f64; 2]) -> [f64; 2] {
    [
        matrix[0][0] * point[0] + matrix[0][1] * point[1] + matrix[0][2],
        matrix[1][0] * point[0] + matrix[1][1] * point[1] + matrix[1][2],
    ]
}

fn linear_geometry(
    transform: [[f64; 3]; 2],
    width: f64,
    height: f64,
    stops: &[(f64, String)],
) -> Option<(f64, Vec<f64>)> {
    let inverse = inverse_transform(transform)?;
    let normalized_start = apply_transform(inverse, [0.0, 0.5]);
    let normalized_end = apply_transform(inverse, [1.0, 0.5]);
    let start = [normalized_start[0] * width, normalized_start[1] * height];
    let end = [normalized_end[0] * width, normalized_end[1] * height];
    let mut figma_angle = (end[1] - start[1]).atan2(end[0] - start[0]).to_degrees() - 90.0;
    figma_angle %= 360.0;
    if figma_angle < 0.0 {
        figma_angle += 360.0;
    }
    let angle = ((figma_angle - 180.0) % 360.0).round();
    let radians = angle.to_radians();
    let half = ((width * radians.sin()).abs() + (height * radians.cos()).abs()) / 2.0;
    let center = [width / 2.0, height / 2.0];
    let css_radians = (angle - 90.0).to_radians();
    let css_start = [
        center[0] - half * css_radians.cos(),
        center[1] - half * css_radians.sin(),
    ];
    let css_end = [
        center[0] + half * css_radians.cos(),
        center[1] + half * css_radians.sin(),
    ];
    let figma_vector = [end[0] - start[0], end[1] - start[1]];
    let css_vector = [css_end[0] - css_start[0], css_end[1] - css_start[1]];
    let denominator = css_vector[0].powi(2) + css_vector[1].powi(2);
    let positions = stops
        .iter()
        .map(|(position, _)| {
            let point = [
                start[0] + figma_vector[0] * position,
                start[1] + figma_vector[1] * position,
            ];
            let relative = [point[0] - css_start[0], point[1] - css_start[1]];
            if denominator == 0.0 {
                0.0
            } else {
                (relative[0] * css_vector[0] + relative[1] * css_vector[1]) / denominator * 100.0
            }
        })
        .collect();
    Some((angle, positions))
}

fn color_from_paint(paint: &Value) -> Option<String> {
    let mut color = paint.get("color")?.clone();
    let alpha = color.get("a").and_then(Value::as_f64).unwrap_or(1.0)
        * paint.get("opacity").and_then(Value::as_f64).unwrap_or(1.0);
    color
        .as_object_mut()?
        .insert("a".to_owned(), Value::from(alpha));
    color_from(&color)
}

pub(super) fn blend_mode(mode: &str) -> Option<String> {
    match mode {
        "PASS_THROUGH" | "NORMAL" => None,
        "LINEAR_BURN" | "LINEAR_DODGE" => None,
        "COLOR_BURN" => Some("color-burn".to_owned()),
        "COLOR_DODGE" => Some("color-dodge".to_owned()),
        "SOFT_LIGHT" => Some("soft-light".to_owned()),
        "HARD_LIGHT" => Some("hard-light".to_owned()),
        value => Some(value.to_ascii_lowercase()),
    }
}

fn bound_color_token(
    view: &TypedNode<'_>,
    variable_tokens: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    let id = view.value("fills")?.as_array()?.iter().find_map(|paint| {
        paint
            .get("boundVariables")?
            .get("color")?
            .get("id")?
            .as_str()
    })?;
    variable_tokens
        .get(id)
        .cloned()
        .or_else(|| (id == "var1").then(|| "primaryColor".to_owned()))
        .or_else(|| Some(id.split([':', '/']).next_back().unwrap_or(id).to_owned()))
}

fn bound_paint_token(
    paint: &Value,
    variable_tokens: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    let id = paint
        .get("boundVariables")?
        .get("color")?
        .get("id")?
        .as_str()?;
    variable_tokens.get(id).cloned()
}

/// Length tokens and the width the screen was drawn for, carried together
/// to every length the style pass writes.
#[derive(Clone, Copy)]
pub(super) struct Lengths<'a> {
    tokens: &'a std::collections::BTreeMap<String, LengthToken>,
    viewport: Option<f64>,
}

impl Lengths<'_> {
    /// `$token` in place of a CSS length `literal`, when every binding the
    /// literal stands for is a variable named for one length token and that
    /// token comes to exactly the literal's value at the width the screen was
    /// drawn for. `None` keeps the literal.
    ///
    /// Exactly, not after rounding: the literal is the drawn value rounded to
    /// two decimals, and a token of `20.004px` compared that way passed for a
    /// literal `20px` it does not render as.
    fn token<'v>(
        self,
        bindings: impl IntoIterator<Item = Option<&'v Value>>,
        literal: f64,
    ) -> Option<String> {
        let mut found: Option<&LengthToken> = None;
        for binding in bindings {
            let token = self.tokens.get(binding_id(binding?)?)?;
            if found.is_some_and(|found| found.token != token.token) {
                return None;
            }
            found = Some(token);
        }
        let token = found?;
        let value = length_at(&token.value, self.viewport?)?;
        (value.is_finite() && value == literal).then(|| format!("${}", token.token))
    }
}

/// The number a CSS length literal says: `12px` is 12, a bare `0` is 0.
fn css_length(literal: &str) -> Option<f64> {
    literal
        .strip_suffix("px")
        .unwrap_or(literal)
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
}

/// The variable a binding names. A text node's binding can be a list, one
/// per run of text; it names a variable when every run names the same one.
fn binding_id(binding: &Value) -> Option<&str> {
    match binding {
        Value::Array(bindings) => {
            let first = binding_id(bindings.first()?)?;
            bindings
                .iter()
                .all(|binding| binding_id(binding) == Some(first))
                .then_some(first)
        }
        binding => binding.get("id")?.as_str(),
    }
}

/// What a length token comes to at a viewport `width`: devup-ui writes slot
/// `i` of a responsive array from `min-width` 0, 480, 768, 992, 1280 and
/// 1600px, and a slot left `null` keeps the one before it.
fn length_at(value: &Value, width: f64) -> Option<f64> {
    const BREAKPOINTS: [f64; 6] = [0.0, 480.0, 768.0, 992.0, 1280.0, 1600.0];
    let length = |value: &Value| value.as_str()?.strip_suffix("px")?.parse::<f64>().ok();
    match value {
        Value::Array(slots) => {
            let slot = BREAKPOINTS.iter().rposition(|min| *min <= width)?;
            slots
                .iter()
                .take(slot + 1)
                .rev()
                .find(|value| !value.is_null())
                .and_then(length)
        }
        value => length(value),
    }
}

/// The width the screen holding `node` was drawn at: the outermost frame
/// collected above it, short of the Section, page or component set it sits
/// in. A length token is checked at that width, as a page shows it.
fn drawn_viewport(snapshot: &Snapshot, node: &RawNode) -> Option<f64> {
    let mut screen = node;
    for _ in 0..snapshot.nodes.len() {
        let Some(parent) = screen
            .typed_view()
            .string("parentId")
            .and_then(|id| snapshot.nodes.get(id))
        else {
            break;
        };
        if matches!(
            parent.typed_view().node_type(),
            "SECTION" | "PAGE" | "DOCUMENT" | "COMPONENT_SET"
        ) {
            break;
        }
        screen = parent;
    }
    screen.typed_view().number("width")
}

/// Writes a length bound to a variable as the variable's `$token` where the
/// token reproduces it: padding, gap, a single corner radius and a font
/// size. A token that is responsive then follows the screen width as the
/// design's breakpoint modes do, where the drawn pixels would hold one
/// width's value at every width. Shadows are done where they are composed.
pub(super) fn tokenize_lengths(
    snapshot: &Snapshot,
    node: &RawNode,
    props: &mut [Prop],
    tokens: &std::collections::BTreeMap<String, LengthToken>,
    used_tokens: &mut BTreeSet<String>,
) {
    if tokens.is_empty() {
        return;
    }
    let view = node.typed_view();
    let lengths = Lengths {
        tokens,
        viewport: drawn_viewport(snapshot, node),
    };
    let grid = view.string("layoutMode") == Some("GRID");
    let single_run = view
        .value("styledTextSegments")
        .and_then(Value::as_array)
        .filter(|runs| runs.len() == 1)
        .and_then(|runs| runs.first());
    let binding = |field: &str| {
        view.value("boundVariables")
            .and_then(|bound| bound.get(field))
            .or_else(|| single_run?.get("boundVariables")?.get(field))
    };
    for (name, PropValue::String(value)) in props.iter_mut() {
        let fields: &[&str] = match name.as_str() {
            "p" => &["paddingTop", "paddingRight", "paddingBottom", "paddingLeft"],
            "px" => &["paddingLeft", "paddingRight"],
            "py" => &["paddingTop", "paddingBottom"],
            "pt" => &["paddingTop"],
            "pr" => &["paddingRight"],
            "pb" => &["paddingBottom"],
            "pl" => &["paddingLeft"],
            "gap" if grid => &["gridRowGap", "gridColumnGap"],
            "gap" => &["itemSpacing"],
            "rowGap" if grid => &["gridRowGap"],
            "columnGap" if grid => &["gridColumnGap"],
            "borderRadius" => &[
                "topLeftRadius",
                "topRightRadius",
                "bottomRightRadius",
                "bottomLeftRadius",
            ],
            "fontSize" if view.node_type() == "TEXT" => &["fontSize"],
            _ => continue,
        };
        let Some(literal) = value.ends_with("px").then(|| css_length(value)).flatten() else {
            continue;
        };
        if let Some(token) = lengths.token(fields.iter().map(|field| binding(field)), literal) {
            used_tokens.insert(token[1..].to_owned());
            *value = token;
        }
    }
}

fn push_radius(view: &TypedNode<'_>, props: &mut Vec<Prop>) {
    if view.node_type() == "ELLIPSE"
        && view
            .value("arcData")
            .and_then(|value| value.get("innerRadius"))
            .and_then(Value::as_f64)
            .is_none_or(|radius| radius == 0.0)
    {
        string_prop(props, "borderRadius", "50%");
        return;
    }
    if let Some(radius) = view.number("cornerRadius")
        && radius != 0.0
    {
        string_prop(props, "borderRadius", px(radius));
        return;
    }
    let radii = [
        view.number("topLeftRadius"),
        view.number("topRightRadius"),
        view.number("bottomRightRadius"),
        view.number("bottomLeftRadius"),
    ];
    if let [Some(a), Some(b), Some(c), Some(d)] = radii {
        if a == 0.0 && b == 0.0 && c == 0.0 && d == 0.0 {
            return;
        }
        let value = if a == b && b == c && c == d {
            px(a)
        } else if a == c && b == d {
            format!("{} {}", px(a), px(b))
        } else if b == d {
            format!("{} {} {}", px(a), px(b), px(c))
        } else {
            [a, b, c, d].map(px).join(" ")
        };
        string_prop(props, "borderRadius", value);
    }
}

/// An inside stroke cannot consume layout space when the padding is too small
/// to absorb a CSS border. Paint it inward with an outline instead.
pub(super) fn inside_stroke_uses_outline(view: &TypedNode<'_>) -> bool {
    view.string("strokeAlign").unwrap_or("INSIDE") == "INSIDE"
        && matches!(view.string("layoutMode"), Some("HORIZONTAL" | "VERTICAL"))
        && view.number("strokeWeight").is_some_and(|weight| {
            weight > 0.0
                && ["paddingTop", "paddingRight", "paddingBottom", "paddingLeft"]
                    .iter()
                    .any(|field| view.number(field).is_some_and(|padding| padding < weight))
        })
}

/// A single straight outside edge is a translated, zero-blur box shadow.
/// Unlike a CSS border it does not reduce a fixed content box or grow a HUG
/// box. Rounded corners, dashes and multi-edge joins need a different paint
/// construction, so retain their existing emission rather than approximate it.
pub(crate) fn single_outside_stroke(view: &TypedNode<'_>) -> Option<(usize, f64)> {
    if view.string("strokeAlign") != Some("OUTSIDE")
        || !matches!(view.string("layoutMode"), Some("HORIZONTAL" | "VERTICAL"))
        || view.number("strokeWeight").is_some()
        || [
            "cornerRadius",
            "topLeftRadius",
            "topRightRadius",
            "bottomRightRadius",
            "bottomLeftRadius",
        ]
        .iter()
        .any(|field| view.number(field).is_some_and(|radius| radius != 0.0))
        || view
            .value("dashPattern")
            .and_then(Value::as_array)
            .is_some_and(|dash| !dash.is_empty())
        || !view
            .value("strokes")
            .and_then(Value::as_array)
            .is_some_and(|paints| {
                // Match push_strokes' selected paint, not a later paint that
                // it will never reach after rejecting an unpaintable first one.
                paints
                    .iter()
                    .find(|paint| {
                        paint.get("visible").and_then(Value::as_bool) != Some(false)
                            && paint.get("type").and_then(Value::as_str) == Some("SOLID")
                    })
                    .is_some_and(|paint| color_from_paint(paint).is_some())
            })
    {
        return None;
    }
    let mut edge = None;
    for (index, field) in [
        "strokeTopWeight",
        "strokeRightWeight",
        "strokeBottomWeight",
        "strokeLeftWeight",
    ]
    .iter()
    .enumerate()
    {
        let weight = view.number(field)?;
        if !weight.is_finite() || weight < 0.0 {
            return None;
        }
        if weight > 0.0 {
            if edge.is_some() {
                return None;
            }
            edge = Some((index, weight));
        }
    }
    edge
}

fn push_strokes(
    view: &TypedNode<'_>,
    outside_edge: Option<(usize, f64)>,
    props: &mut Vec<Prop>,
    used_tokens: &mut BTreeSet<String>,
    variable_tokens: &std::collections::BTreeMap<String, String>,
) {
    let Some(paint) = view
        .value("strokes")
        .and_then(Value::as_array)
        .and_then(|strokes| {
            strokes.iter().find(|paint| {
                paint.get("visible").and_then(Value::as_bool) != Some(false)
                    && paint.get("type").and_then(Value::as_str) == Some("SOLID")
            })
        })
    else {
        return;
    };
    let color = if let Some(token) = bound_paint_token(paint, variable_tokens) {
        used_tokens.insert(token.clone());
        format!("${token}")
    } else if let Some(color) = color_from_paint(paint) {
        color
    } else {
        return;
    };
    let style = if view
        .value("dashPattern")
        .and_then(Value::as_array)
        .is_some_and(|value| !value.is_empty())
    {
        "dashed"
    } else {
        "solid"
    };
    let align = view.string("strokeAlign").unwrap_or("INSIDE");
    let explicit_weight = view.number("strokeWeight");
    if let Some((edge, weight)) = outside_edge {
        let (x, y) = [(0.0, -weight), (weight, 0.0), (0.0, weight), (-weight, 0.0)][edge];
        string_prop(
            props,
            "boxShadow",
            format!("{} {} 0 0 {color}", zero_or_px(x), zero_or_px(y)),
        );
        return;
    }
    if explicit_weight.is_none() {
        let sides = [
            ("strokeTopWeight", "borderTop"),
            ("strokeRightWeight", "borderRight"),
            ("strokeBottomWeight", "borderBottom"),
            ("strokeLeftWeight", "borderLeft"),
        ];
        if sides.iter().all(|(field, _)| view.number(field).is_some()) {
            for (field, prop) in sides {
                let weight = view.number(field).unwrap_or(0.0);
                if weight != 0.0 {
                    string_prop(props, prop, format!("{style} {} {color}", px(weight)));
                }
            }
        }
        return;
    }
    let weight = explicit_weight.unwrap_or(0.0);
    if view.node_type() == "LINE" {
        string_prop(props, "outline", format!("{style} {} {color}", px(weight)));
        let base = if view.string("layoutSizingHorizontal") == Some("FIXED") {
            view.number("width")
                .map(px)
                .unwrap_or_else(|| "100%".to_owned())
        } else {
            "100%".to_owned()
        };
        string_prop(
            props,
            "maxW",
            format!("calc({base} - {})", px(weight * 2.0)),
        );
        if view
            .number("rotation")
            .is_none_or(|rotation| rotation.abs() <= 0.01)
        {
            string_prop(
                props,
                "transform",
                format!("translate({}, {})", px(weight), px(-weight)),
            );
        }
        return;
    }
    if inside_stroke_uses_outline(view) {
        string_prop(props, "outline", format!("{style} {} {color}", px(weight)));
        string_prop(props, "outlineOffset", px(-weight));
    } else if align == "INSIDE" {
        string_prop(props, "border", format!("{style} {} {color}", px(weight)));
    } else {
        string_prop(props, "outline", format!("{style} {} {color}", px(weight)));
        if align == "CENTER" {
            string_prop(props, "outlineOffset", px(-weight / 2.0));
        }
    }
}
