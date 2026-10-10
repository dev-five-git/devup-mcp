//! referencePng over the bridge where other layers overlap the node.
//!
//! `get_screenshot` draws the node in its ancestors' context - their paints,
//! clipping, opacity, blend modes, layer blur and drop shadow - with every
//! other layer left out. A plugin cannot leave a layer out of an export, so it
//! exports the node alone, describes its ancestors (and sends the images their
//! fills use), and this draws the rest. Measured live on the devup Test file
//! against `get_screenshot`: gradient backdrops, a translucent, blurred and
//! shadowed ancestor, a node with its own background blur over a layer behind
//! it, a uniform backdrop with a layer above; each within 2/255 on at least
//! 99.6% of pixels, the rest on text edges and the clipped rim of a blurred
//! ancestor.

use std::collections::HashMap;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::Value;

use crate::{DevupError, ErrorCode};

/// Straight-alpha RGBA in 0..1.
type Px = [f32; 4];

/// Larger areas are not composed: a buffer is 16 bytes a pixel and a deep
/// chain keeps several. The plugin falls back to the canvas export above it.
pub(crate) const MAX_COMPOSE_PIXELS: u64 = 8_000_000;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Scene {
    area: Rect,
    #[serde(default = "one")]
    scale: f64,
    width: u32,
    height: u32,
    /// The page background, 0..255.
    page: [f64; 3],
    /// The node's ancestors below the page, outermost first.
    #[serde(default)]
    layers: Vec<Layer>,
    node: Content,
    /// Image fill bytes by image hash, base64.
    #[serde(default)]
    images: HashMap<String, String>,
}

/// Where an export starts on the canvas; its size is the PNG's.
#[derive(Deserialize, Clone, Copy)]
struct Rect {
    x: f64,
    y: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Layer {
    /// `absoluteTransform`: local to absolute.
    transform: [[f64; 3]; 2],
    width: f64,
    height: f64,
    /// Top-left, top-right, bottom-right, bottom-left.
    #[serde(default)]
    radii: [f64; 4],
    #[serde(default)]
    fills: Vec<Value>,
    #[serde(default = "one")]
    opacity: f64,
    #[serde(default)]
    blend_mode: Option<String>,
    #[serde(default)]
    clips: bool,
    #[serde(default)]
    effects: Vec<Value>,
    /// The mask that clips this layer among its siblings.
    #[serde(default)]
    mask: Option<Picture>,
}

/// A plugin export placed at the absolute bounds it covers.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Picture {
    data: String,
    area: Rect,
    #[serde(default)]
    blend_mode: Option<String>,
    /// A LUMINANCE mask clips by brightness, the others by alpha.
    #[serde(default)]
    luminance: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Content {
    /// The node alone.
    #[serde(default)]
    picture: Option<Picture>,
    /// The node drawn from its own paints, when its export cannot be used
    /// alone (a background blur that sampled a layer behind it)...
    #[serde(default)]
    own: Option<Layer>,
    /// ...with its children's exports on top.
    #[serde(default)]
    children: Vec<Picture>,
    /// The mask that clips the node among its siblings.
    #[serde(default)]
    mask: Option<Picture>,
}

fn one() -> f64 {
    1.0
}

fn invalid(message: impl Into<String>) -> DevupError {
    DevupError::new(
        ErrorCode::DevupSnapshotUnsupported,
        format!(
            "the plugin's reference PNG scene cannot be composed: {}",
            message.into()
        ),
        false,
    )
}

/// The composed picture as a base64 PNG, opaque, `width` x `height`.
pub(crate) fn compose(scene: &Value) -> Result<String, DevupError> {
    let scene = Scene::deserialize(scene).map_err(|error| invalid(error.to_string()))?;
    let (width, height) = (scene.width as usize, scene.height as usize);
    if width == 0 || height == 0 || (width as u64) * (height as u64) > MAX_COMPOSE_PIXELS {
        return Err(invalid(format!(
            "{width}x{height} is outside the composable range"
        )));
    }
    if !(scene.scale.is_finite() && scene.scale > 0.0 && scene.scale <= 64.0) {
        return Err(invalid(format!(
            "the scale {} is outside the composable range",
            scene.scale
        )));
    }
    // A blur at the area's rim reads what lies past it - the ancestors'
    // paints go on there - so the stack is drawn wider and cut back.
    let mut margin = blur_reach(&scene);
    while margin > 0
        && ((width + 2 * margin) * (height + 2 * margin)) as u64 > 2 * MAX_COMPOSE_PIXELS
    {
        margin /= 2;
    }
    let wide = width + 2 * margin;
    let mut images = Images::new(&scene.images)?;
    within_budget(&scene, wide, height + 2 * margin, &images)?;
    let pixels = draw(&scene, wide, height + 2 * margin, margin, &mut images)?;
    let mut rgba = Vec::with_capacity(width * height * 4);
    for row in pixels.chunks_exact(wide).skip(margin).take(height) {
        for pixel in &row[margin..margin + width] {
            for channel in &pixel[..3] {
                rgba.push((channel.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
            rgba.push(255);
        }
    }
    let image = image::RgbaImage::from_raw(scene.width, scene.height, rgba)
        .ok_or_else(|| invalid("the pixel buffer does not fill the image"))?;
    let mut encoded = Vec::new();
    image
        .write_with_encoder(image::codecs::png::PngEncoder::new(&mut encoded))
        .map_err(|error| invalid(error.to_string()))?;
    Ok(STANDARD.encode(encoded))
}

struct Grid {
    width: usize,
    height: usize,
    origin: (f64, f64),
    scale: f64,
}

impl Grid {
    /// The absolute point at a pixel's centre. Pixels sit on whole canvas
    /// units from the floor of the bounds' start; an export is the ceiling of
    /// the bounds' size wide, as `get_screenshot`'s is. Starting at the exact,
    /// fractional start instead matched `get_screenshot` worse on two of the
    /// three composed nodes that start between pixels.
    fn point(&self, index: usize) -> (f64, f64) {
        let (column, row) = (index % self.width, index / self.width);
        (
            self.origin.0 + (column as f64 + 0.5) / self.scale,
            self.origin.1 + (row as f64 + 0.5) / self.scale,
        )
    }

    fn origin_of(rect: Rect, scale: f64) -> (f64, f64) {
        (
            (rect.x * scale).floor() / scale,
            (rect.y * scale).floor() / scale,
        )
    }
}

struct Entry<'a> {
    pixels: Vec<Px>,
    clip: Vec<f32>,
    layer: Option<&'a Layer>,
    /// The mask that cuts this layer where it is laid back.
    mask: Option<Vec<Px>>,
}

/// Working memory one composition may take: the stack's buffers and masks,
/// those a layer, blur or picture holds while it is drawn, and the decoded
/// pictures and images. Past it the area is drawn as the canvas shows it
/// instead. The answer the scene came in is read before this is counted.
const MAX_COMPOSE_BYTES: u64 = 1536 * 1024 * 1024;

/// What a decoder may hold besides its per-pixel buffers: row buffers and
/// metadata chunks. Counted once: pictures and images decode one at a time.
const DECODE_SLACK: u64 = 16 * 1024 * 1024;

/// Kernel taps across every blur and shadow, so a wide blur over a large area
/// is drawn as the canvas shows it rather than computed for minutes.
const MAX_BLUR_TAPS: u64 = 4_000_000_000;

/// Refuses, before drawing, what would take more memory or blur work than one
/// reference PNG is worth.
fn within_budget(
    scene: &Scene,
    width: usize,
    height: usize,
    images: &Images,
) -> Result<(), DevupError> {
    let pixels = (width as u64) * (height as u64);
    let layers = || scene.layers.iter().chain(scene.node.own.as_ref());
    let isolated = layers().filter(|layer| isolating(layer)).count() as u64
        + u64::from(scene.node.own.is_some() && scene.node.mask.is_some());
    let masks = layers().filter(|layer| layer.mask.is_some()).count() as u64
        + u64::from(scene.node.mask.is_some());
    // Pictures are decoded one at a time, each dropped once laid on the grid.
    let mut largest_picture = 0_u64;
    for picture in layers()
        .filter_map(|layer| layer.mask.as_ref())
        .chain(&scene.node.mask)
        .chain(&scene.node.picture)
        .chain(&scene.node.children)
    {
        let (_, cost) = decode_cost(&base64_bytes(&picture.data)?)?;
        largest_picture = largest_picture.max(cost);
    }
    // 20 bytes a pixel for each stack entry (colour and clip), 16 for each
    // mask kept to cut its layer, and about 120 for what one layer, blur or
    // picture holds while it is drawn.
    let bytes = pixels
        .saturating_mul(20 * (1 + isolated) + 16 * masks + 120)
        .saturating_add(largest_picture)
        .saturating_add(DECODE_SLACK)
        .saturating_add(images.decoded_bytes());
    if bytes > MAX_COMPOSE_BYTES {
        return Err(invalid(format!(
            "it would take {} MiB to draw",
            bytes >> 20
        )));
    }
    let taps = layers()
        .flat_map(|layer| layer.effects.iter().filter(|effect| visible(effect)))
        .filter(|effect| {
            matches!(
                effect["type"].as_str(),
                Some("LAYER_BLUR" | "BACKGROUND_BLUR" | "DROP_SHADOW")
            )
        })
        .map(|effect| {
            let reach = (number(&effect["radius"]) * scene.scale * 1.5)
                .ceil()
                .max(0.0) as u64;
            pixels.saturating_mul(reach.saturating_mul(4).saturating_add(2))
        })
        .fold(0_u64, u64::saturating_add);
    if taps > MAX_BLUR_TAPS {
        return Err(invalid(format!("its blurs would take {taps} kernel taps")));
    }
    Ok(())
}

/// How many pixels past the area the widest blur or shadow in the scene reads.
fn blur_reach(scene: &Scene) -> usize {
    scene
        .layers
        .iter()
        .chain(scene.node.own.as_ref())
        .flat_map(|layer| layer.effects.iter().filter(|effect| visible(effect)))
        .map(|effect| {
            let sigma = number(&effect["radius"]) * scene.scale / 2.0;
            let shift = if effect["type"] == "DROP_SHADOW" {
                number(&effect["offset"]["x"])
                    .abs()
                    .max(number(&effect["offset"]["y"]).abs())
                    * scene.scale
            } else {
                0.0
            };
            (sigma * 3.0 + shift).ceil() as usize
        })
        .max()
        .unwrap_or(0)
        .min(512)
}

fn draw(
    scene: &Scene,
    width: usize,
    height: usize,
    margin: usize,
    images: &mut Images,
) -> Result<Vec<Px>, DevupError> {
    let origin = Grid::origin_of(scene.area, scene.scale);
    let grid = Grid {
        width,
        height,
        origin: (
            origin.0 - margin as f64 / scene.scale,
            origin.1 - margin as f64 / scene.scale,
        ),
        scale: scene.scale,
    };
    let count = width * height;
    let page = [
        (scene.page[0] / 255.0) as f32,
        (scene.page[1] / 255.0) as f32,
        (scene.page[2] / 255.0) as f32,
        1.0,
    ];
    let mut stack = vec![Entry {
        pixels: vec![page; count],
        clip: vec![1.0; count],
        layer: None,
        mask: None,
    }];
    let own = scene.node.own.as_ref();
    for layer in scene.layers.iter().chain(own) {
        // Drawn from its parts, the node takes its own mask: it cuts the
        // node's paints, background blur and children alike.
        let is_own = own.is_some_and(|own| std::ptr::eq(own, layer));
        let mask = match layer
            .mask
            .as_ref()
            .or(scene.node.mask.as_ref().filter(|_| is_own))
        {
            Some(mask) => Some(place(mask, &grid, true)?),
            None => None,
        };
        let geometry = Geometry::new(layer)?;
        let shape: Vec<f32> = (0..count)
            .map(|index| geometry.coverage(grid.point(index), grid.scale))
            .collect();
        let isolated = isolating(layer) || mask.is_some();
        if isolated {
            // The ancestors' clip cuts this layer's result - after its blur
            // and shadow - where it is laid back, not its content before them.
            stack.push(Entry {
                pixels: vec![[0.0; 4]; count],
                clip: vec![1.0; count],
                layer: Some(layer),
                mask,
            });
        }
        if let Some(radius) = effect_radius(layer, "BACKGROUND_BLUR") {
            let below = if isolated {
                stack.len() - 2
            } else {
                stack.len() - 1
            };
            let blurred = gaussian(
                &stack[below].pixels,
                width,
                height,
                radius * scene.scale / 2.0,
            );
            let (lower, upper) = stack.split_at_mut(below + 1);
            let entry = &mut lower[below];
            let layer_mask = upper.last().and_then(|top| top.mask.as_deref());
            for index in 0..count {
                let cut = layer_mask.map_or(1.0, |mask| mask[index][3]);
                let weight = shape[index] * entry.clip[index] * cut;
                entry.pixels[index] = mix(entry.pixels[index], blurred[index], weight);
            }
        }
        let top = stack.last_mut().expect("the page entry stays");
        for paint in layer.fills.iter().filter(|paint| visible(paint)) {
            let blend = Blend::parse(paint["blendMode"].as_str())?;
            let cells = top.pixels.iter_mut().zip(shape.iter().zip(&top.clip));
            for (index, (pixel, (&cover, &clip))) in cells.enumerate() {
                let coverage = cover * clip;
                if coverage <= 0.0 {
                    continue;
                }
                let local = geometry.local(grid.point(index));
                let source = paint_at(paint, layer, local, images)?;
                composite(pixel, source, blend, coverage);
            }
        }
        if layer.clips {
            for (clip, &cover) in top.clip.iter_mut().zip(&shape) {
                *clip *= cover;
            }
        }
    }
    let top = stack.last_mut().expect("the page entry stays");
    // Drawn from its parts, the node's mask already went with its paints.
    let node_mask = match (&scene.node.mask, own) {
        (Some(mask), None) => Some(place(mask, &grid, true)?),
        _ => None,
    };
    if let Some(picture) = &scene.node.picture {
        let blend = Blend::parse(picture.blend_mode.as_deref())?;
        let placed = place(picture, &grid, false)?;
        for index in 0..count {
            let coverage = top.clip[index] * node_mask.as_ref().map_or(1.0, |mask| mask[index][3]);
            composite(&mut top.pixels[index], placed[index], blend, coverage);
        }
    }
    for child in &scene.node.children {
        let blend = Blend::parse(child.blend_mode.as_deref())?;
        let placed = place(child, &grid, false)?;
        for ((pixel, &source), &clip) in top.pixels.iter_mut().zip(&placed).zip(&top.clip) {
            composite(pixel, source, blend, clip);
        }
    }
    while stack.len() > 1 {
        let entry = stack.pop().expect("more than the page entry");
        let layer = entry.layer.expect("an isolated entry names its layer");
        let mut pixels = entry.pixels;
        for effect in layer.effects.iter().filter(|effect| visible(effect)) {
            if effect["type"] == "DROP_SHADOW" {
                pixels = with_drop_shadow(&pixels, &grid, effect);
            }
        }
        if let Some(radius) = effect_radius(layer, "LAYER_BLUR") {
            pixels = gaussian(&pixels, width, height, radius * scene.scale / 2.0);
        }
        let mask = entry.mask;
        let blend = Blend::parse(layer.blend_mode.as_deref())?;
        let below = stack.last_mut().expect("the page entry stays");
        for index in 0..count {
            let mut source = pixels[index];
            source[3] *= layer.opacity as f32 * mask.as_ref().map_or(1.0, |mask| mask[index][3]);
            composite(&mut below.pixels[index], source, blend, below.clip[index]);
        }
    }
    Ok(stack.pop().expect("the page entry stays").pixels)
}

fn visible(item: &Value) -> bool {
    item.is_object() && item["visible"] != false
}

fn isolating(layer: &Layer) -> bool {
    layer.opacity < 1.0
        || !matches!(
            layer.blend_mode.as_deref(),
            None | Some("PASS_THROUGH" | "NORMAL")
        )
        || layer.mask.is_some()
        || layer.effects.iter().any(|effect| {
            visible(effect) && matches!(effect["type"].as_str(), Some("LAYER_BLUR" | "DROP_SHADOW"))
        })
}

fn effect_radius(layer: &Layer, kind: &str) -> Option<f64> {
    layer
        .effects
        .iter()
        .filter(|effect| visible(effect) && effect["type"] == kind)
        .filter_map(|effect| effect["radius"].as_f64())
        .fold(None, |largest: Option<f64>, radius| {
            Some(largest.map_or(radius, |r| r.max(radius)))
        })
}

/// A layer's transform inverted, its size and corner radii.
struct Geometry {
    inverse: [[f64; 2]; 2],
    translate: (f64, f64),
    width: f64,
    height: f64,
    radii: [f64; 4],
}

impl Geometry {
    fn new(layer: &Layer) -> Result<Self, DevupError> {
        let [[a, b, tx], [c, d, ty]] = layer.transform;
        let determinant = a * d - b * c;
        if determinant.abs() < 1e-12 || layer.width <= 0.0 || layer.height <= 0.0 {
            return Err(invalid("a layer has a degenerate transform or size"));
        }
        let limit = layer.width.min(layer.height) / 2.0;
        Ok(Self {
            inverse: [
                [d / determinant, -b / determinant],
                [-c / determinant, a / determinant],
            ],
            translate: (tx, ty),
            width: layer.width,
            height: layer.height,
            radii: layer.radii.map(|radius| radius.clamp(0.0, limit)),
        })
    }

    fn local(&self, (x, y): (f64, f64)) -> (f64, f64) {
        let (dx, dy) = (x - self.translate.0, y - self.translate.1);
        (
            self.inverse[0][0] * dx + self.inverse[0][1] * dy,
            self.inverse[1][0] * dx + self.inverse[1][1] * dy,
        )
    }

    /// Anti-aliased coverage of the (rounded) rectangle at an absolute point.
    fn coverage(&self, point: (f64, f64), scale: f64) -> f32 {
        let (x, y) = self.local(point);
        let half = 0.5 / scale;
        let edge = x.min(self.width - x).min(y).min(self.height - y);
        let mut coverage = ((edge + half) / (2.0 * half)).clamp(0.0, 1.0);
        let [tl, tr, br, bl] = self.radii;
        for (radius, cx, cy, sx, sy) in [
            (tl, tl, tl, -1.0, -1.0),
            (tr, self.width - tr, tr, 1.0, -1.0),
            (br, self.width - br, self.height - br, 1.0, 1.0),
            (bl, bl, self.height - bl, -1.0, 1.0),
        ] {
            if radius > 0.0 && (x - cx) * sx > 0.0 && (y - cy) * sy > 0.0 {
                let distance = (x - cx).hypot(y - cy) - radius;
                coverage = coverage.min(((-distance + half) / (2.0 * half)).clamp(0.0, 1.0));
            }
        }
        coverage as f32
    }
}

fn number(value: &Value) -> f64 {
    value.as_f64().unwrap_or(0.0)
}

fn colour(value: &Value, alpha: f64) -> Px {
    [
        number(&value["r"]) as f32,
        number(&value["g"]) as f32,
        number(&value["b"]) as f32,
        (value["a"].as_f64().unwrap_or(1.0) * alpha) as f32,
    ]
}

fn paint_at(
    paint: &Value,
    layer: &Layer,
    (x, y): (f64, f64),
    images: &mut Images,
) -> Result<Px, DevupError> {
    let opacity = paint["opacity"].as_f64().unwrap_or(1.0);
    let (nx, ny) = (x / layer.width, y / layer.height);
    let kind = paint["type"].as_str().unwrap_or_default();
    match kind {
        "SOLID" => {
            let mut px = colour(&paint["color"], 1.0);
            px[3] = opacity as f32;
            Ok(px)
        }
        "GRADIENT_LINEAR" | "GRADIENT_RADIAL" | "GRADIENT_ANGULAR" | "GRADIENT_DIAMOND" => {
            let m = &paint["gradientTransform"];
            let row = |r: usize, c: usize| number(&m[r][c]);
            let qx = row(0, 0) * nx + row(0, 1) * ny + row(0, 2);
            let qy = row(1, 0) * nx + row(1, 1) * ny + row(1, 2);
            let t = match kind {
                "GRADIENT_LINEAR" => qx,
                "GRADIENT_RADIAL" => (qx - 0.5).hypot(qy - 0.5) * 2.0,
                "GRADIENT_DIAMOND" => ((qx - 0.5).abs() + (qy - 0.5).abs()) * 2.0,
                _ => ((qy - 0.5).atan2(qx - 0.5) / std::f64::consts::TAU).rem_euclid(1.0),
            }
            .clamp(0.0, 1.0);
            let mut px = gradient_colour(&paint["gradientStops"], t)?;
            px[3] *= opacity as f32;
            Ok(px)
        }
        "IMAGE" => {
            let hash = paint["imageHash"].as_str().unwrap_or_default();
            let image = images.get(hash)?;
            let (iw, ih) = (image.width as f64, image.height as f64);
            let (lw, lh) = (layer.width, layer.height);
            let (u, v, inside) = match paint["scaleMode"].as_str().unwrap_or("FILL") {
                "FILL" | "FIT" => {
                    let s = if paint["scaleMode"] == "FIT" {
                        (lw / iw).min(lh / ih)
                    } else {
                        (lw / iw).max(lh / ih)
                    };
                    let u = (x - (lw - iw * s) / 2.0) / s;
                    let v = (y - (lh - ih * s) / 2.0) / s;
                    (u, v, (0.0..iw).contains(&u) && (0.0..ih).contains(&v))
                }
                "TILE" => {
                    let s = paint["scalingFactor"]
                        .as_f64()
                        .filter(|s| *s > 0.0)
                        .unwrap_or(1.0);
                    ((x / s).rem_euclid(iw), (y / s).rem_euclid(ih), true)
                }
                _ => {
                    let m = &paint["imageTransform"];
                    let row = |r: usize, c: usize| number(&m[r][c]);
                    let u = (row(0, 0) * nx + row(0, 1) * ny + row(0, 2)) * iw;
                    let v = (row(1, 0) * nx + row(1, 1) * ny + row(1, 2)) * ih;
                    (u, v, (0.0..iw).contains(&u) && (0.0..ih).contains(&v))
                }
            };
            let mut px = image.bilinear(u - 0.5, v - 0.5);
            px[3] *= (opacity * f64::from(u8::from(inside))) as f32;
            Ok(px)
        }
        other => Err(invalid(format!("a {other} paint"))),
    }
}

fn gradient_colour(stops: &Value, t: f64) -> Result<Px, DevupError> {
    let mut stops: Vec<(f64, Px)> = stops
        .as_array()
        .ok_or_else(|| invalid("a gradient without stops"))?
        .iter()
        .map(|stop| (number(&stop["position"]), colour(&stop["color"], 1.0)))
        .collect();
    if stops.is_empty() {
        return Err(invalid("a gradient without stops"));
    }
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    let first = stops[0];
    let last = stops[stops.len() - 1];
    if t <= first.0 {
        return Ok(first.1);
    }
    if t >= last.0 {
        return Ok(last.1);
    }
    let pair = stops
        .windows(2)
        .find(|pair| t >= pair[0].0 && t <= pair[1].0)
        .ok_or_else(|| invalid("gradient stops out of order"))?;
    let span = (pair[1].0 - pair[0].0).max(1e-12);
    let k = ((t - pair[0].0) / span) as f32;
    let mut px = [0.0; 4];
    for (channel, value) in px.iter_mut().enumerate() {
        *value = pair[0].1[channel] + (pair[1].1[channel] - pair[0].1[channel]) * k;
    }
    Ok(px)
}

/// A decoded picture, RGBA bytes: a quarter of what straight floats take.
struct Decoded {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
}

fn base64_bytes(data: &str) -> Result<Vec<u8>, DevupError> {
    STANDARD
        .decode(data.as_bytes())
        .map_err(|_| invalid("an image is not base64"))
}

impl Decoded {
    /// Decoded within what the budget counted for it (`decode_cost`).
    fn from_bytes(bytes: &[u8]) -> Result<Self, DevupError> {
        let (decoder, _) = decode_cost(bytes)?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(decoder.saturating_add(DECODE_SLACK));
        let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|error| invalid(format!("an image cannot be read: {error}")))?;
        reader.limits(limits);
        let image = reader
            .decode()
            .map_err(|error| invalid(format!("an image does not decode: {error}")))?
            .into_rgba8();
        Ok(Self {
            width: image.width() as usize,
            height: image.height() as usize,
            rgba: image.into_raw(),
        })
    }

    fn at(&self, x: usize, y: usize) -> Px {
        let start = (y * self.width + x) * 4;
        let mut px = [0.0; 4];
        for (value, &byte) in px.iter_mut().zip(&self.rgba[start..start + 4]) {
            *value = f32::from(byte) / 255.0;
        }
        px
    }

    fn bilinear(&self, u: f64, v: f64) -> Px {
        let u = u.clamp(0.0, (self.width - 1) as f64);
        let v = v.clamp(0.0, (self.height - 1) as f64);
        let (x0, y0) = (u.floor() as usize, v.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.width - 1), (y0 + 1).min(self.height - 1));
        let (fx, fy) = ((u - x0 as f64) as f32, (v - y0 as f64) as f32);
        let (a, b, c, d) = (
            self.at(x0, y0),
            self.at(x1, y0),
            self.at(x0, y1),
            self.at(x1, y1),
        );
        let mut px = [0.0; 4];
        for (channel, value) in px.iter_mut().enumerate() {
            let top = a[channel] * (1.0 - fx) + b[channel] * fx;
            let bottom = c[channel] * (1.0 - fx) + d[channel] * fx;
            *value = top * (1.0 - fy) + bottom * fy;
        }
        px
    }
}

/// The images the ancestors' fills use, read once: their bytes up front, so
/// their size is known before anything is drawn, and their pixels when first
/// sampled.
struct Images {
    sources: HashMap<String, Vec<u8>>,
    cost: u64,
    decoded: HashMap<String, Decoded>,
}

/// An image's size from its header, without decoding it.
fn dimensions(encoded: &[u8]) -> Result<(u32, u32), DevupError> {
    image::ImageReader::new(std::io::Cursor::new(encoded))
        .with_guessed_format()
        .map_err(|error| invalid(format!("an image cannot be read: {error}")))?
        .into_dimensions()
        .map_err(|error| invalid(format!("an image cannot be read: {error}")))
}

/// What decoding an image may allocate, from its header alone: what its
/// decoder may allocate, and that with the RGBA8 it becomes (4 bytes a pixel)
/// and a copy of its bytes.
///
/// PNG and GIF decoders are held to their part (`Decoded::from_bytes`): a
/// 16-bit PNG decodes to 8 bytes a pixel, and a GIF frame larger than its
/// screen is refused. The others take no such limit, so they are counted from
/// what sizes their buffers:
/// - A JPEG decoder keeps two bytes for each sample of each component's
///   coefficients, at the sampling factors its frame headers name, over rows
///   and columns padded to whole MCUs, beside a 4-byte output.
/// - A WebP decoder sizes its buffers from its image data before checking it
///   against the canvas, so a WebP whose data does not match its canvas or
///   frames, or whose chunks run past it, is refused. One that matches takes
///   at most 12 bytes a pixel: canvas, frame and planes.
fn decode_cost(encoded: &[u8]) -> Result<(u64, u64), DevupError> {
    let (width, height) = dimensions(encoded)?;
    let (width, height) = (u64::from(width), u64::from(height));
    let decoder = match image::guess_format(encoded) {
        Ok(image::ImageFormat::Jpeg) => {
            let coefficients = jpeg_coefficient_bytes(encoded)
                .ok_or_else(|| invalid("a JPEG's frame header cannot be read"))?;
            (width + 32)
                .saturating_mul(height + 32)
                .saturating_mul(coefficients + 4)
        }
        Ok(image::ImageFormat::WebP) if !webp_matches_its_canvas(encoded, width, height) => {
            return Err(invalid("a WebP's image data does not match its canvas"));
        }
        Ok(image::ImageFormat::WebP) => width.saturating_mul(height).saturating_mul(12),
        _ => width.saturating_mul(height).saturating_mul(8),
    };
    let kept = width.saturating_mul(height).saturating_mul(4);
    Ok((
        decoder,
        decoder
            .saturating_add(kept)
            .saturating_add(encoded.len() as u64),
    ))
}

/// Two bytes a pixel for each sample of each component, at the largest
/// sampling factors any frame header before the first scan names.
fn jpeg_coefficient_bytes(encoded: &[u8]) -> Option<u64> {
    let mut largest = None;
    let mut at = 2;
    while let Some(&byte) = encoded.get(at) {
        if byte != 0xFF {
            return None;
        }
        let marker = *encoded.get(at + 1)?;
        match marker {
            0xFF => at += 1,
            0x01 | 0xD0..=0xD9 => at += 2,
            0xDA => break,
            _ => {
                let length = u16::from_be_bytes([*encoded.get(at + 2)?, *encoded.get(at + 3)?]);
                if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                    let components = usize::from(*encoded.get(at + 9)?);
                    let samples: Option<u64> = (0..components)
                        .map(|index| {
                            let factors = *encoded.get(at + 11 + 3 * index)?;
                            Some(2 * u64::from(factors >> 4) * u64::from(factors & 15))
                        })
                        .sum();
                    largest = largest.max(Some(samples?));
                }
                at += 2 + usize::from(length);
            }
        }
    }
    largest
}

/// Whether a WebP's chunks lie within it and each image it holds is the size
/// it claims: the canvas for a still image, its frame for an animated one,
/// each frame inside the canvas.
fn webp_matches_its_canvas(encoded: &[u8], width: u64, height: u64) -> bool {
    fn chunks(data: &[u8]) -> Option<Vec<(&[u8], &[u8])>> {
        let mut found = Vec::new();
        let mut at = 0;
        while at < data.len() {
            let header = data.get(at..at + 8)?;
            let size = u32::from_le_bytes(header[4..8].try_into().ok()?) as usize;
            found.push((&header[..4], data.get(at + 8..at + 8 + size)?));
            at += 8 + size + size % 2;
        }
        Some(found)
    }
    fn size_of(kind: &[u8], body: &[u8]) -> Option<(u64, u64)> {
        match kind {
            b"VP8 " => Some((
                u64::from(u16::from_le_bytes([*body.get(6)?, *body.get(7)?]) & 0x3FFF),
                u64::from(u16::from_le_bytes([*body.get(8)?, *body.get(9)?]) & 0x3FFF),
            )),
            b"VP8L" => {
                let bits = u32::from_le_bytes(body.get(1..5)?.try_into().ok()?);
                Some((
                    u64::from(bits & 0x3FFF) + 1,
                    u64::from((bits >> 14) & 0x3FFF) + 1,
                ))
            }
            _ => None,
        }
    }
    let fits = |chunks: &[(&[u8], &[u8])], width: u64, height: u64| {
        chunks.iter().all(|(kind, body)| {
            !matches!(*kind, b"VP8 " | b"VP8L") || size_of(kind, body) == Some((width, height))
        })
    };
    let riff = encoded
        .get(4..8)
        .and_then(|size| Some(u32::from_le_bytes(size.try_into().ok()?) as usize));
    let (Some(b"RIFF"), Some(b"WEBP"), Some(riff)) = (encoded.get(..4), encoded.get(8..12), riff)
    else {
        return false;
    };
    let Some(top) = encoded.get(12..8 + riff).and_then(chunks) else {
        return false;
    };
    fits(&top, width, height)
        && top
            .iter()
            .filter(|(kind, _)| *kind == b"ANMF")
            .all(|(_, frame)| {
                let field = |at: usize| {
                    frame
                        .get(at..at + 3)
                        .map(|b| u64::from(b[0]) | u64::from(b[1]) << 8 | u64::from(b[2]) << 16)
                };
                let (Some(x), Some(y), Some(w), Some(h)) = (field(0), field(3), field(6), field(9))
                else {
                    return false;
                };
                let (w, h) = (w + 1, h + 1);
                2 * x + w <= width
                    && 2 * y + h <= height
                    && frame
                        .get(16..)
                        .and_then(chunks)
                        .is_some_and(|inner| fits(&inner, w, h))
            })
}

impl Images {
    fn new(sources: &HashMap<String, String>) -> Result<Self, DevupError> {
        let mut bytes = HashMap::new();
        let mut cost = 0_u64;
        for (hash, data) in sources {
            let encoded = base64_bytes(data)?;
            cost = cost.saturating_add(decode_cost(&encoded)?.1);
            bytes.insert(hash.clone(), encoded);
        }
        Ok(Self {
            sources: bytes,
            cost,
            decoded: HashMap::new(),
        })
    }

    /// What decoding them all takes: each is kept once decoded.
    fn decoded_bytes(&self) -> u64 {
        self.cost
    }

    fn get(&mut self, hash: &str) -> Result<&Decoded, DevupError> {
        if !self.decoded.contains_key(hash) {
            let data = self
                .sources
                .get(hash)
                .ok_or_else(|| invalid(format!("the image {hash} was not sent")))?;
            self.decoded
                .insert(hash.to_owned(), Decoded::from_bytes(data)?);
        }
        Ok(&self.decoded[hash])
    }
}

/// A plugin export laid on the grid; outside it, transparent. A mask reads as
/// its coverage in the alpha channel.
fn place(picture: &Picture, grid: &Grid, as_mask: bool) -> Result<Vec<Px>, DevupError> {
    let image = Decoded::from_bytes(&base64_bytes(&picture.data)?)?;
    let origin = Grid::origin_of(picture.area, grid.scale);
    let offset_x = ((origin.0 - grid.origin.0) * grid.scale).round() as isize;
    let offset_y = ((origin.1 - grid.origin.1) * grid.scale).round() as isize;
    let mut out = vec![[0.0; 4]; grid.width * grid.height];
    for (index, px) in out.iter_mut().enumerate() {
        let column = (index % grid.width) as isize - offset_x;
        let row = (index / grid.width) as isize - offset_y;
        if column < 0 || row < 0 || column as usize >= image.width || row as usize >= image.height {
            continue;
        }
        let source = image.at(column as usize, row as usize);
        *px = if as_mask && picture.luminance {
            let luma = 0.2126 * source[0] + 0.7152 * source[1] + 0.0722 * source[2];
            [0.0, 0.0, 0.0, luma * source[3]]
        } else {
            source
        };
    }
    Ok(out)
}

fn mix(a: Px, b: Px, weight: f32) -> Px {
    let mut out = [0.0; 4];
    for channel in 0..4 {
        out[channel] = a[channel] + (b[channel] - a[channel]) * weight;
    }
    out
}

/// A drop shadow behind the layer: its alpha, moved, blurred and coloured.
fn with_drop_shadow(pixels: &[Px], grid: &Grid, effect: &Value) -> Vec<Px> {
    let colour = colour(&effect["color"], 1.0);
    let shift = |value: &Value| (number(value) * grid.scale).round().clamp(-1e9, 1e9) as isize;
    let (dx, dy) = (shift(&effect["offset"]["x"]), shift(&effect["offset"]["y"]));
    let (width, height) = (grid.width, grid.height);
    let mut shadow = vec![[0.0; 4]; width * height];
    for (index, px) in shadow.iter_mut().enumerate() {
        let column = (index % width) as isize - dx;
        let row = (index / width) as isize - dy;
        if column >= 0 && row >= 0 && (column as usize) < width && (row as usize) < height {
            let alpha = pixels[row as usize * width + column as usize][3];
            *px = [colour[0], colour[1], colour[2], alpha * colour[3]];
        }
    }
    let shadow = gaussian(
        &shadow,
        width,
        height,
        number(&effect["radius"]) * grid.scale / 2.0,
    );
    let mut out = shadow;
    for (index, px) in out.iter_mut().enumerate() {
        composite(px, pixels[index], Blend::Normal, 1.0);
    }
    out
}

/// Separable Gaussian on premultiplied colour; nothing outside the grid.
fn gaussian(pixels: &[Px], width: usize, height: usize, sigma: f64) -> Vec<Px> {
    if sigma < 0.05 {
        return pixels.to_vec();
    }
    let reach = (sigma * 3.0).ceil() as isize;
    let weights: Vec<f32> = (-reach..=reach)
        .map(|offset| (-((offset * offset) as f64) / (2.0 * sigma * sigma)).exp() as f32)
        .collect();
    let total: f32 = weights.iter().sum();
    let weights: Vec<f32> = weights.iter().map(|weight| weight / total).collect();
    let premultiplied: Vec<Px> = pixels
        .iter()
        .map(|p| [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]])
        .collect();
    let pass = |source: &[Px], horizontal: bool| -> Vec<Px> {
        let mut out = vec![[0.0; 4]; width * height];
        for (index, px) in out.iter_mut().enumerate() {
            let (column, row) = ((index % width) as isize, (index / width) as isize);
            for (k, weight) in weights.iter().enumerate() {
                let offset = k as isize - reach;
                let (c, r) = if horizontal {
                    (column + offset, row)
                } else {
                    (column, row + offset)
                };
                if c < 0 || r < 0 || c as usize >= width || r as usize >= height {
                    continue;
                }
                let sample = source[r as usize * width + c as usize];
                for channel in 0..4 {
                    px[channel] += sample[channel] * weight;
                }
            }
        }
        out
    };
    pass(&pass(&premultiplied, true), false)
        .into_iter()
        .map(|p| {
            if p[3] > 1e-6 {
                [p[0] / p[3], p[1] / p[3], p[2] / p[3], p[3]]
            } else {
                [0.0; 4]
            }
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Blend {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    LinearDodge,
    LinearBurn,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl Blend {
    fn parse(mode: Option<&str>) -> Result<Self, DevupError> {
        Ok(match mode.unwrap_or("NORMAL") {
            "NORMAL" | "PASS_THROUGH" => Self::Normal,
            "MULTIPLY" => Self::Multiply,
            "SCREEN" => Self::Screen,
            "OVERLAY" => Self::Overlay,
            "DARKEN" => Self::Darken,
            "LIGHTEN" => Self::Lighten,
            "COLOR_DODGE" => Self::ColorDodge,
            "COLOR_BURN" => Self::ColorBurn,
            "HARD_LIGHT" => Self::HardLight,
            "SOFT_LIGHT" => Self::SoftLight,
            "DIFFERENCE" => Self::Difference,
            "EXCLUSION" => Self::Exclusion,
            "LINEAR_DODGE" => Self::LinearDodge,
            "LINEAR_BURN" => Self::LinearBurn,
            "HUE" => Self::Hue,
            "SATURATION" => Self::Saturation,
            "COLOR" => Self::Color,
            "LUMINOSITY" => Self::Luminosity,
            other => return Err(invalid(format!("the blend mode {other}"))),
        })
    }

    fn channel(self, b: f32, s: f32) -> f32 {
        match self {
            Self::Multiply => b * s,
            Self::Screen => b + s - b * s,
            Self::Overlay => Self::HardLight.channel(s, b),
            Self::Darken => b.min(s),
            Self::Lighten => b.max(s),
            Self::ColorDodge if b <= 0.0 => 0.0,
            Self::ColorDodge if s >= 1.0 => 1.0,
            Self::ColorDodge => (b / (1.0 - s)).min(1.0),
            Self::ColorBurn if b >= 1.0 => 1.0,
            Self::ColorBurn if s <= 0.0 => 0.0,
            Self::ColorBurn => 1.0 - ((1.0 - b) / s).min(1.0),
            Self::HardLight if s <= 0.5 => b * 2.0 * s,
            Self::HardLight => Self::Screen.channel(b, 2.0 * s - 1.0),
            Self::SoftLight => {
                let d = if b <= 0.25 {
                    ((16.0 * b - 12.0) * b + 4.0) * b
                } else {
                    b.sqrt()
                };
                if s <= 0.5 {
                    b - (1.0 - 2.0 * s) * b * (1.0 - b)
                } else {
                    b + (2.0 * s - 1.0) * (d - b)
                }
            }
            Self::Difference => (b - s).abs(),
            Self::Exclusion => b + s - 2.0 * b * s,
            Self::LinearDodge => (b + s).min(1.0),
            Self::LinearBurn => (b + s - 1.0).max(0.0),
            _ => s,
        }
    }

    /// W3C compositing: separable modes per channel, the four others on the
    /// whole colour.
    fn apply(self, b: [f32; 3], s: [f32; 3]) -> [f32; 3] {
        match self {
            Self::Normal => s,
            Self::Hue => set_lum(set_sat(s, sat(b)), lum(b)),
            Self::Saturation => set_lum(set_sat(b, sat(s)), lum(b)),
            Self::Color => set_lum(s, lum(b)),
            Self::Luminosity => set_lum(b, lum(s)),
            _ => [0, 1, 2].map(|channel| self.channel(b[channel], s[channel])),
        }
    }
}

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    let c = c.map(|channel| channel + d);
    let l = lum(c);
    let (n, x) = (c[0].min(c[1]).min(c[2]), c[0].max(c[1]).max(c[2]));
    c.map(|channel| {
        let mut channel = channel;
        if n < 0.0 {
            channel = l + (channel - l) * l / (l - n).max(1e-9);
        }
        if x > 1.0 {
            channel = l + (channel - l) * (1.0 - l) / (x - l).max(1e-9);
        }
        channel
    })
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let (n, x) = (c[0].min(c[1]).min(c[2]), c[0].max(c[1]).max(c[2]));
    if x <= n {
        return [0.0; 3];
    }
    c.map(|channel| (channel - n) / (x - n) * s)
}

/// Source over destination with a blend mode, straight alpha in and out.
fn composite(dst: &mut Px, src: Px, blend: Blend, coverage: f32) {
    let sa = src[3] * coverage;
    if sa <= 0.0 {
        return;
    }
    let da = dst[3];
    let (cs, cb) = ([src[0], src[1], src[2]], [dst[0], dst[1], dst[2]]);
    let blended = blend.apply(cb, cs);
    let out_a = sa + da * (1.0 - sa);
    for channel in 0..3 {
        let mixed = (1.0 - da) * cs[channel] + da * blended[channel];
        dst[channel] = (sa * mixed + (1.0 - sa) * da * cb[channel]) / out_a.max(1e-9);
    }
    dst[3] = out_a;
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn png(width: u32, height: u32, rgba: Vec<u8>) -> String {
        let image = image::RgbaImage::from_raw(width, height, rgba).expect("pixels fill the image");
        let mut encoded = Vec::new();
        image
            .write_with_encoder(image::codecs::png::PngEncoder::new(&mut encoded))
            .expect("a PNG encodes");
        STANDARD.encode(encoded)
    }

    fn decode(data: &str) -> Vec<u8> {
        image::load_from_memory(&STANDARD.decode(data).expect("base64"))
            .expect("a PNG")
            .to_rgba8()
            .into_raw()
    }

    fn frame(fills: Value) -> Value {
        json!({"transform": [[1, 0, 0], [0, 1, 0]], "width": 4, "height": 1, "fills": fills, "clips": true})
    }

    /// The node over a horizontal gradient its parent is painted with: the
    /// gradient runs left to right across the parent, sampled at pixel centres.
    #[test]
    fn a_node_alone_lands_on_its_parents_gradient() {
        let scene = json!({
            "area": {"x": 0, "y": 0, "width": 4, "height": 1}, "width": 4, "height": 1, "page": [30, 30, 30],
            "layers": [frame(json!([{"type": "GRADIENT_LINEAR", "gradientTransform": [[1, 0, 0], [0, 1, 0]],
                "gradientStops": [{"position": 0, "color": {"r": 0, "g": 0, "b": 0, "a": 1}},
                                  {"position": 1, "color": {"r": 1, "g": 1, "b": 1, "a": 1}}]}]))],
            "node": {"picture": {"data": png(4, 1, vec![255, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
                                 "area": {"x": 0, "y": 0, "width": 4, "height": 1}}}
        });
        let rgba = decode(&compose(&scene).expect("composes"));
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
        assert_eq!(&rgba[4..8], &[96, 96, 96, 255]);
        assert_eq!(&rgba[12..16], &[223, 223, 223, 255]);
    }

    /// A translucent ancestor fades the node with it, and a MULTIPLY node
    /// multiplies what lies under it.
    #[test]
    fn group_opacity_and_blend_modes_apply_to_the_node() {
        let node = png(4, 1, [128, 128, 128, 255].repeat(4));
        let scene = |opacity: f64, blend: &str| {
            json!({
                "area": {"x": 0, "y": 0, "width": 4, "height": 1}, "width": 4, "height": 1, "page": [255, 255, 255],
                "layers": [{"transform": [[1, 0, 0], [0, 1, 0]], "width": 4, "height": 1, "opacity": opacity,
                            "fills": [{"type": "SOLID", "color": {"r": 0, "g": 0, "b": 1}}]}],
                "node": {"picture": {"data": node, "area": {"x": 0, "y": 0, "width": 4, "height": 1}, "blendMode": blend}}
            })
        };
        let faded = decode(&compose(&scene(0.25, "NORMAL")).expect("composes"));
        assert_eq!(&faded[0..4], &[223, 223, 223, 255]);
        let multiplied = decode(&compose(&scene(1.0, "MULTIPLY")).expect("composes"));
        assert_eq!(&multiplied[0..4], &[0, 0, 128, 255]);
    }

    /// A mask clips the node to its own alpha; outside the mask nothing of the
    /// node shows.
    #[test]
    fn a_mask_clips_the_node() {
        let scene = json!({
            "area": {"x": 0, "y": 0, "width": 2, "height": 1}, "width": 2, "height": 1, "page": [0, 0, 0],
            "node": {"picture": {"data": png(2, 1, [255, 255, 255, 255].repeat(2)),
                                 "area": {"x": 0, "y": 0, "width": 2, "height": 1}},
                     "mask": {"data": png(1, 1, vec![0, 0, 0, 255]), "area": {"x": 1, "y": 0, "width": 1, "height": 1}}}
        });
        let rgba = decode(&compose(&scene).expect("composes"));
        assert_eq!(rgba, vec![0, 0, 0, 255, 255, 255, 255, 255]);
    }

    /// A background blur over a backdrop that goes on past the area keeps the
    /// rim the same as the middle: the blur reads that backdrop, not nothing.
    #[test]
    fn a_background_blur_reads_the_backdrop_past_the_area() {
        let scene = json!({
            "area": {"x": 0, "y": 0}, "width": 2, "height": 2, "page": [0, 0, 0],
            "layers": [{"transform": [[1, 0, -20], [0, 1, -20]], "width": 42, "height": 42,
                        "fills": [{"type": "SOLID", "color": {"r": 1, "g": 0, "b": 0}}]}],
            "node": {"own": {"transform": [[1, 0, 0], [0, 1, 0]], "width": 2, "height": 2,
                             "fills": [{"type": "SOLID", "opacity": 0.2, "color": {"r": 1, "g": 1, "b": 1}}],
                             "effects": [{"type": "BACKGROUND_BLUR", "radius": 4}]}}
        });
        let rgba = decode(&compose(&scene).expect("composes"));
        assert_eq!(rgba, [255, 51, 51, 255].repeat(4));
    }

    /// A clipping ancestor cuts a translucent layer once, where the layer is
    /// laid back: half covered at the clip's edge and at half opacity is a
    /// quarter, not an eighth.
    #[test]
    fn a_clip_cuts_an_isolated_layer_once() {
        let scene = json!({
            "area": {"x": 0, "y": 0}, "width": 2, "height": 1, "page": [0, 0, 0],
            "layers": [
                {"transform": [[1, 0, 0], [0, 1, 0]], "width": 1.5, "height": 1, "clips": true},
                {"transform": [[1, 0, 0], [0, 1, 0]], "width": 2, "height": 1, "opacity": 0.5,
                 "fills": [{"type": "SOLID", "color": {"r": 1, "g": 1, "b": 1}}]}
            ],
            "node": {"picture": {"data": png(2, 1, vec![0; 8]), "area": {"x": 0, "y": 0}}}
        });
        let rgba = decode(&compose(&scene).expect("composes"));
        assert_eq!(rgba, vec![128, 128, 128, 255, 64, 64, 64, 255]);
    }

    /// Drawn from its parts (its own paints, then its children), the node is
    /// still cut by its mask: outside it neither shows, inside both do.
    #[test]
    fn a_mask_cuts_a_node_drawn_from_its_parts() {
        let scene = json!({
            "area": {"x": 0, "y": 0}, "width": 2, "height": 1, "page": [0, 0, 0],
            "node": {
                "own": {"transform": [[1, 0, 0], [0, 1, 0]], "width": 2, "height": 1,
                        "fills": [{"type": "SOLID", "color": {"r": 1, "g": 1, "b": 1}}],
                        "effects": [{"type": "BACKGROUND_BLUR", "radius": 2}]},
                "children": [{"data": png(1, 1, vec![255, 0, 0, 255]), "area": {"x": 1, "y": 0}}],
                "mask": {"data": png(1, 1, vec![0, 0, 0, 255]), "area": {"x": 1, "y": 0}}
            }
        });
        let rgba = decode(&compose(&scene).expect("composes"));
        assert_eq!(rgba, vec![0, 0, 0, 255, 255, 0, 0, 255]);
    }

    /// A PNG that is only its header, claiming `width` x `height`.
    fn png_header(width: u32, height: u32) -> String {
        fn crc(bytes: &[u8]) -> u32 {
            let mut crc = !0_u32;
            for &byte in bytes {
                crc ^= u32::from(byte);
                for _ in 0..8 {
                    crc = if crc & 1 == 1 {
                        (crc >> 1) ^ 0xEDB8_8320
                    } else {
                        crc >> 1
                    };
                }
            }
            !crc
        }
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut chunk = |kind: &[u8; 4], data: &[u8]| {
            let typed = [kind.as_slice(), data].concat();
            png.extend((data.len() as u32).to_be_bytes());
            png.extend(&typed);
            png.extend(crc(&typed).to_be_bytes());
        };
        chunk(
            b"IHDR",
            &[width.to_be_bytes(), height.to_be_bytes()]
                .concat()
                .iter()
                .copied()
                .chain([8, 6, 0, 0, 0])
                .collect::<Vec<u8>>(),
        );
        chunk(b"IDAT", &[]);
        chunk(b"IEND", &[]);
        STANDARD.encode(png)
    }

    /// Every mask a layer keeps to cut it counts: eleven masked ancestors over
    /// 2000x2000 take over 1.5 GiB though their stack alone was counted under it.
    #[test]
    fn masks_kept_on_the_stack_count_toward_the_budget() {
        let masked = |_| {
            json!({"transform": [[1, 0, 0], [0, 1, 0]], "width": 2000, "height": 2000,
            "mask": {"data": png(1, 1, vec![0, 0, 0, 255]), "area": {"x": 0, "y": 0}}})
        };
        let scene = json!({
            "area": {"x": 0, "y": 0}, "width": 2000, "height": 2000, "page": [0, 0, 0],
            "layers": (0..11).map(masked).collect::<Vec<_>>(),
            "node": {"picture": {"data": png(1, 1, vec![0, 0, 0, 0]), "area": {"x": 0, "y": 0}}}
        });
        let error = compose(&scene).expect_err("too much memory");
        assert!(error.message.contains("MiB to draw"), "{}", error.message);
    }

    /// A picture's size is read from its header before it is decoded: one that
    /// would take too much to decode is refused, not decoded.
    #[test]
    fn a_picture_too_large_to_decode_is_refused_from_its_header() {
        let scene = json!({
            "area": {"x": 0, "y": 0}, "width": 2000, "height": 2000, "page": [0, 0, 0],
            "node": {"picture": {"data": png_header(10_000, 10_000), "area": {"x": 0, "y": 0}}}
        });
        let error = compose(&scene).expect_err("too much memory");
        assert!(error.message.contains("MiB to draw"), "{}", error.message);
    }

    /// The mask of a node drawn from its parts is sized from its header too.
    #[test]
    fn the_mask_of_a_node_drawn_from_its_parts_is_sized_up_front() {
        let scene = json!({
            "area": {"x": 0, "y": 0}, "width": 2000, "height": 2000, "page": [0, 0, 0],
            "node": {"own": {"transform": [[1, 0, 0], [0, 1, 0]], "width": 2000, "height": 2000,
                             "mask": {"data": png_header(10_000, 10_000), "area": {"x": 0, "y": 0}}}}
        });
        let error = compose(&scene).expect_err("too much memory");
        assert!(error.message.contains("MiB to draw"), "{}", error.message);
    }

    /// A GIF whose first frame is far larger than the 1x1 screen its header
    /// names is refused by the decoder before that frame is allocated.
    #[test]
    fn a_gif_frame_larger_than_its_header_is_not_decoded() {
        let mut gif = b"GIF89a".to_vec();
        gif.extend([1, 0, 1, 0, 0, 0, 0]);
        gif.push(0x2C);
        gif.extend([0, 0, 0, 0]);
        gif.extend(10_000_u16.to_le_bytes());
        gif.extend(12_000_u16.to_le_bytes());
        gif.extend([0x80, 0, 0, 0, 255, 255, 255, 2, 0, 0x3B]);
        let scene = json!({
            "area": {"x": 0, "y": 0}, "width": 1, "height": 1, "page": [0, 0, 0],
            "node": {"picture": {"data": STANDARD.encode(gif), "area": {"x": 0, "y": 0}}}
        });
        let error = compose(&scene).expect_err("the frame is refused");
        assert!(
            error.message.contains("Memory limit exceeded"),
            "{}",
            error.message
        );
    }

    /// A WebP whose 1x1 canvas holds a VP8 frame claiming 16383x16383 is
    /// refused before its decoder sizes planes from that frame.
    #[test]
    fn a_webp_whose_data_does_not_match_its_canvas_is_not_decoded() {
        let mut webp = b"RIFF".to_vec();
        webp.extend(40_u32.to_le_bytes());
        webp.extend(b"WEBPVP8X");
        webp.extend(10_u32.to_le_bytes());
        webp.extend([0; 10]);
        webp.extend(b"VP8 ");
        webp.extend(10_u32.to_le_bytes());
        webp.extend([0, 0, 0, 0x9d, 0x01, 0x2a, 0xff, 0x3f, 0xff, 0x3f]);
        let scene = json!({
            "area": {"x": 0, "y": 0}, "width": 1, "height": 1, "page": [0, 0, 0],
            "node": {"picture": {"data": STANDARD.encode(webp), "area": {"x": 0, "y": 0}}}
        });
        let error = compose(&scene).expect_err("the WebP is refused");
        assert!(
            error.message.contains("does not match its canvas"),
            "{}",
            error.message
        );
    }

    /// A JPEG is counted at the sampling factors its frame header names.
    #[test]
    fn a_jpeg_is_counted_at_its_sampling_factors() {
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
        jpeg.extend([0xFF, 0xC0, 0x00, 0x14, 8, 0x0F, 0xA0, 0x0F, 0xA0, 4]);
        for id in 1..=4 {
            jpeg.extend([id, 0x44, 0]);
        }
        jpeg.extend([0xFF, 0xDA]);
        assert_eq!(jpeg_coefficient_bytes(&jpeg), Some(2 * 16 * 4));
    }

    /// Image fills in JPEG and WebP still compose.
    #[test]
    fn jpeg_and_webp_fills_compose() {
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode(
                &[255, 0, 0].repeat(16),
                4,
                4,
                image::ExtendedColorType::Rgb8,
            )
            .expect("a JPEG encodes");
        let mut webp = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut webp)
            .encode(
                &[0, 0, 255, 255].repeat(16),
                4,
                4,
                image::ExtendedColorType::Rgba8,
            )
            .expect("a WebP encodes");
        for (bytes, channel) in [(jpeg, 0), (webp, 2)] {
            let scene = json!({
                "area": {"x": 0, "y": 0}, "width": 4, "height": 4, "page": [0, 0, 0],
                "layers": [{"transform": [[1, 0, 0], [0, 1, 0]], "width": 4, "height": 4,
                            "fills": [{"type": "IMAGE", "imageHash": "fill", "scaleMode": "FILL"}]}],
                "node": {"picture": {"data": png(1, 1, vec![0, 0, 0, 0]), "area": {"x": 0, "y": 0}}},
                "images": {"fill": STANDARD.encode(bytes)}
            });
            let rgba = decode(&compose(&scene).expect("composes"));
            assert!(rgba[channel] > 200, "{rgba:?}");
        }
    }

    /// What would take more memory or blur work than a reference PNG is worth
    /// is refused before it is drawn, so the canvas picture is used.
    #[test]
    fn an_oversized_composition_is_refused_up_front() {
        let translucent = |_| json!({"transform": [[1, 0, 0], [0, 1, 0]], "width": 2000, "height": 2000, "opacity": 0.5});
        let deep = json!({
            "area": {"x": 0, "y": 0}, "width": 2000, "height": 2000, "page": [0, 0, 0],
            "layers": (0..60).map(translucent).collect::<Vec<_>>(),
            "node": {"picture": {"data": png(1, 1, vec![0, 0, 0, 0]), "area": {"x": 0, "y": 0}}}
        });
        let error = compose(&deep).expect_err("too much memory");
        assert!(error.message.contains("MiB to draw"), "{}", error.message);

        let blurred = json!({
            "area": {"x": 0, "y": 0}, "width": 2000, "height": 2000, "page": [0, 0, 0],
            "layers": [{"transform": [[1, 0, 0], [0, 1, 0]], "width": 2000, "height": 2000,
                        "effects": [{"type": "LAYER_BLUR", "radius": 1e12}]}],
            "node": {"picture": {"data": png(1, 1, vec![0, 0, 0, 0]), "area": {"x": 0, "y": 0}}}
        });
        let error = compose(&blurred).expect_err("too much blur work");
        assert!(error.message.contains("kernel taps"), "{}", error.message);

        let unscaled = json!({
            "area": {"x": 0, "y": 0}, "scale": 0, "width": 1, "height": 1, "page": [0, 0, 0],
            "node": {"picture": {"data": png(1, 1, vec![0, 0, 0, 0]), "area": {"x": 0, "y": 0}}}
        });
        assert!(compose(&unscaled).is_err());
    }

    #[test]
    fn an_unknown_paint_is_refused_not_guessed() {
        let scene = json!({
            "area": {"x": 0, "y": 0, "width": 1, "height": 1}, "width": 1, "height": 1, "page": [0, 0, 0],
            "layers": [{"transform": [[1, 0, 0], [0, 1, 0]], "width": 1, "height": 1, "fills": [{"type": "VIDEO"}]}],
            "node": {"picture": {"data": png(1, 1, vec![0, 0, 0, 0]), "area": {"x": 0, "y": 0, "width": 1, "height": 1}}}
        });
        assert!(compose(&scene).is_err());
    }
}
