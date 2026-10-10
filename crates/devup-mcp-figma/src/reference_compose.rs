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
    images: HashMap<String, ImageSource>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ImageSource {
    /// Its own bytes, laid out at its own size.
    Bytes(String),
    /// A PNG the plugin's UI decoded it into (the original was not a PNG, JPEG
    /// or GIF, or too large), laid out at the original's size.
    Decoded {
        data: String,
        width: f64,
        height: f64,
    },
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
    /// Drawn over all the layer holds: its paints, inside, outside or across
    /// its edge by `stroke_align`, `stroke_weights` wide (top, right, bottom,
    /// left). The plugin sends only a rectangle's undashed, mitred strokes.
    #[serde(default)]
    strokes: Vec<Value>,
    #[serde(default)]
    stroke_align: Option<String>,
    #[serde(default)]
    stroke_weights: [f64; 4],
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
    let stroked = layers().filter(|layer| !layer.strokes.is_empty()).count() as u64;
    // Pictures and images decode one at a time; a picture is dropped once laid
    // on the grid, an image is kept.
    let mut peak = images.peak;
    for picture in layers()
        .filter_map(|layer| layer.mask.as_ref())
        .chain(&scene.node.mask)
        .chain(&scene.node.picture)
        .chain(&scene.node.children)
    {
        let encoded = base64_bytes(&picture.data)?;
        peak = peak.max(Header::read(&encoded)?.peak(encoded.len()));
    }
    // 20 bytes a pixel for each stack entry (colour and clip), 16 for each
    // mask kept to cut its layer, 4 for the clip a stroke waits with, and about
    // 120 for what one layer, blur or picture holds while it is drawn.
    let bytes = pixels
        .saturating_mul(20 * (1 + isolated) + 16 * masks + 4 * stroked + 120)
        .saturating_add(peak)
        .saturating_add(images.kept);
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
    // A layer's stroke goes over everything it holds, so it waits until that
    // is drawn, with the clip that cuts the layer itself.
    let mut strokes: Vec<(usize, &Layer, Vec<f32>)> = Vec::new();
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
        let depth = stack.len() - 1;
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
        if !layer.strokes.is_empty() {
            strokes.push((depth, layer, top.clip.clone()));
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
    loop {
        // The entry on top is complete: lay the strokes of the layers drawn
        // into it, innermost first.
        let depth = stack.len() - 1;
        let top = stack.last_mut().expect("the page entry stays");
        for (_, layer, clip) in strokes.iter().rev().filter(|(at, ..)| *at == depth) {
            stroke(&mut top.pixels, layer, clip, &grid, images)?;
        }
        if stack.len() == 1 {
            break;
        }
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

/// A rectangle's stroke laid over `pixels`, cut by `clip`.
fn stroke(
    pixels: &mut [Px],
    layer: &Layer,
    clip: &[f32],
    grid: &Grid,
    images: &mut Images,
) -> Result<(), DevupError> {
    let geometry = Geometry::new(layer)?;
    let [top, right, bottom, left] = layer.stroke_weights;
    let half = [top / 2.0, right / 2.0, bottom / 2.0, left / 2.0];
    let (outer, inner) = match layer.stroke_align.as_deref() {
        Some("INSIDE") => ([0.0; 4], [-top, -right, -bottom, -left]),
        Some("OUTSIDE") => (layer.stroke_weights, [0.0; 4]),
        _ => (half, half.map(|weight| -weight)),
    };
    for paint in layer.strokes.iter().filter(|paint| visible(paint)) {
        let blend = Blend::parse(paint["blendMode"].as_str())?;
        for (index, pixel) in pixels.iter_mut().enumerate() {
            let point = grid.point(index);
            let ring = (geometry.grown(point, grid.scale, outer)
                - geometry.grown(point, grid.scale, inner))
            .max(0.0);
            let coverage = ring * clip[index];
            if coverage <= 0.0 {
                continue;
            }
            let source = paint_at(paint, layer, geometry.local(point), images)?;
            composite(pixel, source, blend, coverage);
        }
    }
    Ok(())
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

    /// Anti-aliased coverage of the rectangle grown by `grow` on each side (top,
    /// right, bottom, left; negative shrinks). A rounded corner's arc grows
    /// about the same centre; a square corner stays square, as a mitre does.
    fn grown(&self, point: (f64, f64), scale: f64, [top, right, bottom, left]: [f64; 4]) -> f32 {
        let (x, y) = self.local(point);
        let half = 0.5 / scale;
        let (x0, y0, x1, y1) = (-left, -top, self.width + right, self.height + bottom);
        if x1 <= x0 || y1 <= y0 {
            return 0.0;
        }
        let edge = (x - x0).min(x1 - x).min(y - y0).min(y1 - y);
        let mut coverage = ((edge + half) / (2.0 * half)).clamp(0.0, 1.0);
        let [tl, tr, br, bl] = self.radii;
        for (radius, grow, cx, cy, sx, sy) in [
            (tl, top.max(left), tl, tl, -1.0, -1.0),
            (tr, top.max(right), self.width - tr, tr, 1.0, -1.0),
            (
                br,
                bottom.max(right),
                self.width - br,
                self.height - br,
                1.0,
                1.0,
            ),
            (bl, bottom.max(left), bl, self.height - bl, -1.0, 1.0),
        ] {
            let arc = radius + grow;
            if radius > 0.0 && arc > 0.0 && (x - cx) * sx > 0.0 && (y - cy) * sy > 0.0 {
                let distance = (x - cx).hypot(y - cy) - arc;
                coverage = coverage.min(((-distance + half) / (2.0 * half)).clamp(0.0, 1.0));
            }
        }
        coverage as f32
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
            let (iw, ih) = image.layout;
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
            // Exactly 1 unless the plugin sent the image smaller than laid out.
            let (sx, sy) = (image.width as f64 / iw, image.height as f64 / ih);
            let mut px = image.bilinear(u * sx - 0.5, v * sy - 0.5);
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
    /// The size its fill is laid out at: its own, or the original's when the
    /// plugin sent it decoded and smaller.
    layout: (f64, f64),
}

fn base64_bytes(data: &str) -> Result<Vec<u8>, DevupError> {
    STANDARD
        .decode(data.as_bytes())
        .map_err(|_| invalid("an image is not base64"))
}

impl Decoded {
    /// Decoded as the format its header names, within its allowance.
    fn from_bytes(bytes: &[u8]) -> Result<Self, DevupError> {
        let header = Header::read(bytes)?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(header.allowance);
        let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes));
        reader.set_format(header.format);
        reader.limits(limits);
        let image = reader
            .decode()
            .map_err(|error| invalid(format!("an image does not decode: {error}")))?
            .into_rgba8();
        Ok(Self {
            width: image.width() as usize,
            height: image.height() as usize,
            layout: (f64::from(image.width()), f64::from(image.height())),
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
    /// The original's size, for an image the plugin sent decoded.
    layouts: HashMap<String, (f64, f64)>,
    /// What stays once they are decoded, and the most one decode takes.
    kept: u64,
    peak: u64,
    decoded: HashMap<String, Decoded>,
}

/// A picture or image read from its first bytes without a decoder, so nothing
/// is allocated for one the budget then refuses.
struct Header {
    format: image::ImageFormat,
    pixels: u64,
    /// What its decoder may allocate.
    allowance: u64,
    /// What its decoder keeps for each PNG text chunk, which no allowance holds.
    texts: u64,
}

/// What a PNG decoder keeps for one text chunk beside its bytes: the entry and
/// its strings.
const PNG_TEXT_CHUNK_BYTES: u64 = 512;

/// PNGs wider than this are not decoded. A PNG decoder unfilters rows outside
/// its allowance, and for a wide interlaced 16-bit image those buffers outgrow
/// it; up to this width (the plugin's exports' longest edge) they stay within
/// `DECODE_SLACK`.
const MAX_PNG_WIDTH: u64 = 8192;

impl Header {
    /// Its format, size and allowance:
    /// - PNG and GIF: 8 bytes a pixel (a 16-bit PNG decodes to 8), which
    ///   `Decoded::from_bytes` holds their decoders to; a GIF frame larger than
    ///   its screen decodes only within it. A PNG's text chunks are counted
    ///   one by one.
    /// - JPEG: two bytes for each sample of each component's coefficients at
    ///   the sampling factors its frame header names, over rows and columns
    ///   padded to whole MCUs, and a 4-byte output. Its decoder takes no
    ///   allocation limit; this is what it is counted at.
    /// - Anything else, WebP among them, is refused: its decoder takes no limit
    ///   and sizes buffers from data no header bounds, its entropy tables and
    ///   partitions among them.
    fn read(encoded: &[u8]) -> Result<Self, DevupError> {
        let field =
            |at: usize, length: usize| at.checked_add(length).and_then(|end| encoded.get(at..end));
        let (format, width, height, allowance, texts) = if encoded.starts_with(b"\x89PNG\r\n\x1a\n")
        {
            let size =
                |at| field(at, 4).map(|b| u64::from(u32::from_be_bytes([b[0], b[1], b[2], b[3]])));
            let (Some(b"IHDR"), Some(width), Some(height)) = (field(12, 4), size(16), size(20))
            else {
                return Err(invalid("a PNG's header cannot be read"));
            };
            if width > MAX_PNG_WIDTH {
                return Err(invalid(format!(
                    "a PNG wider than {MAX_PNG_WIDTH} pixels is not composed"
                )));
            }
            let mut texts = 0_u64;
            let mut at = 8_usize;
            while at < encoded.len() {
                let (Some(length), Some(kind)) = (size(at), field(at + 4, 4)) else {
                    return Err(invalid("a PNG's chunks cannot be read"));
                };
                let Some(end) = usize::try_from(length)
                    .ok()
                    .and_then(|length| at.checked_add(12)?.checked_add(length))
                    .filter(|&end| end <= encoded.len())
                else {
                    return Err(invalid("a PNG's chunk runs past its end"));
                };
                if matches!(kind, b"tEXt" | b"zTXt" | b"iTXt") {
                    texts += 1;
                }
                if kind == b"IEND" {
                    break;
                }
                at = end;
            }
            (
                image::ImageFormat::Png,
                width,
                height,
                width.saturating_mul(height).saturating_mul(8),
                texts,
            )
        } else if encoded.starts_with(b"GIF87a") || encoded.starts_with(b"GIF89a") {
            let size = |at| field(at, 2).map(|b| u64::from(u16::from_le_bytes([b[0], b[1]])));
            let (Some(width), Some(height)) = (size(6), size(8)) else {
                return Err(invalid("a GIF's header cannot be read"));
            };
            (
                image::ImageFormat::Gif,
                width,
                height,
                width * height * 8,
                0,
            )
        } else if encoded.starts_with(&[0xFF, 0xD8]) {
            let (width, height, coefficients) = jpeg_frame(encoded)
                .ok_or_else(|| invalid("a JPEG's frame header cannot be read"))?;
            let padded = (width + 32).saturating_mul(height + 32);
            (
                image::ImageFormat::Jpeg,
                width,
                height,
                padded.saturating_mul(coefficients + 4),
                0,
            )
        } else {
            return Err(invalid("an image is not a PNG, JPEG or GIF"));
        };
        if width == 0 || height == 0 {
            return Err(invalid("an image has no size"));
        }
        Ok(Self {
            format,
            pixels: width.saturating_mul(height),
            allowance: allowance.saturating_add(DECODE_SLACK),
            texts: texts.saturating_mul(PNG_TEXT_CHUNK_BYTES),
        })
    }

    /// The most decoding it takes: its allowance three times (the output it is
    /// held to, what a PNG decoder keeps within its own copy of the allowance,
    /// and the rows it unfilters outside it), the RGBA8 it becomes, eight
    /// copies of its bytes (the source, a JPEG decoder's own copy, the metadata
    /// it gathers and reassembles, and the lists holding it), and its text
    /// chunks.
    fn peak(&self, encoded: usize) -> u64 {
        self.allowance
            .saturating_mul(3)
            .saturating_add(self.pixels.saturating_mul(4))
            .saturating_add((encoded as u64).saturating_mul(8))
            .saturating_add(self.texts)
    }
}

/// A JPEG's size and two bytes a pixel for each sample of each component, from
/// its frame header: the first, before its first scan (its decoder refuses a
/// second). A marker standing alone before it (another SOI, a restart, a
/// stuffed zero) or a segment too short for its length is refused: its decoder
/// reads those differently, and could skip the frame header counted here.
fn jpeg_frame(encoded: &[u8]) -> Option<(u64, u64, u64)> {
    let mut at = 2;
    loop {
        if *encoded.get(at)? != 0xFF {
            return None;
        }
        let marker = *encoded.get(at + 1)?;
        let word = |offset: usize| {
            Some(u16::from_be_bytes([
                *encoded.get(at + offset)?,
                *encoded.get(at + offset + 1)?,
            ]))
        };
        match marker {
            0xFF => at += 1,
            0x00 | 0x01 | 0xD0..=0xDA => return None,
            0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                let (height, width) = (u64::from(word(5)?), u64::from(word(7)?));
                let components = usize::from(*encoded.get(at + 9)?);
                if usize::from(word(2)?) != 8 + 3 * components {
                    return None;
                }
                let samples = (0..components)
                    .map(|index| {
                        let factors = *encoded.get(at + 11 + 3 * index)?;
                        Some(2 * u64::from(factors >> 4) * u64::from(factors & 15))
                    })
                    .sum::<Option<u64>>()?;
                return Some((width, height, samples));
            }
            _ => {
                let length = usize::from(word(2)?);
                if length < 2 {
                    return None;
                }
                at += 2 + length;
            }
        }
    }
}

impl Images {
    fn new(sources: &HashMap<String, ImageSource>) -> Result<Self, DevupError> {
        let (mut bytes, mut layouts) = (HashMap::new(), HashMap::new());
        let (mut kept, mut peak) = (0_u64, 0_u64);
        for (hash, source) in sources {
            let (data, layout) = match source {
                ImageSource::Bytes(data) => (data, None),
                ImageSource::Decoded {
                    data,
                    width,
                    height,
                } if *width > 0.0 && *height > 0.0 => (data, Some((*width, *height))),
                ImageSource::Decoded { .. } => {
                    return Err(invalid(format!("the image {hash} has no size")));
                }
            };
            let encoded = base64_bytes(data)?;
            let header = Header::read(&encoded)?;
            kept = kept
                .saturating_add(header.pixels.saturating_mul(4))
                .saturating_add(encoded.len() as u64);
            peak = peak.max(header.peak(encoded.len()));
            bytes.insert(hash.clone(), encoded);
            if let Some(layout) = layout {
                layouts.insert(hash.clone(), layout);
            }
        }
        Ok(Self {
            sources: bytes,
            layouts,
            kept,
            peak,
            decoded: HashMap::new(),
        })
    }

    fn get(&mut self, hash: &str) -> Result<&Decoded, DevupError> {
        if !self.decoded.contains_key(hash) {
            let data = self
                .sources
                .get(hash)
                .ok_or_else(|| invalid(format!("the image {hash} was not sent")))?;
            let mut decoded = Decoded::from_bytes(data)?;
            if let Some(layout) = self.layouts.get(hash) {
                decoded.layout = *layout;
            }
            self.decoded.insert(hash.to_owned(), decoded);
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
            "node": {"picture": {"data": png_header(8_192, 12_000), "area": {"x": 0, "y": 0}}}
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
                             "mask": {"data": png_header(8_192, 12_000), "area": {"x": 0, "y": 0}}}}
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

    /// A WebP is not decoded: its decoder takes no allocation limit and sizes
    /// buffers from data no header bounds. This one hides a VP8 frame claiming
    /// 16383x16383 past the RIFF size its 1x1 canvas declares.
    #[test]
    fn a_webp_is_not_decoded() {
        let mut webp = b"RIFF".to_vec();
        webp.extend(22_u32.to_le_bytes());
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
            error.message.contains("not a PNG, JPEG or GIF"),
            "{}",
            error.message
        );
    }

    /// A JPEG is read from its frame header: its size, and its coefficients at
    /// the sampling factors it names.
    #[test]
    fn a_jpeg_is_counted_at_its_sampling_factors() {
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
        jpeg.extend([0xFF, 0xC0, 0x00, 0x14, 8, 0x0F, 0xA0, 0x0F, 0xA0, 4]);
        for id in 1..=4 {
            jpeg.extend([id, 0x44, 0]);
        }
        jpeg.extend([0xFF, 0xDA]);
        assert_eq!(jpeg_frame(&jpeg), Some((4000, 4000, 2 * 16 * 4)));
    }

    /// A PNG's text chunks are counted one by one: its decoder keeps an entry
    /// for each that its allowance does not hold.
    #[test]
    fn a_pngs_text_chunks_are_counted() {
        let mut bytes = STANDARD
            .decode(png(1, 1, vec![0, 0, 0, 0]))
            .expect("base64");
        let text = [0, 0, 0, 2, b't', b'E', b'X', b't', b'a', 0, 0, 0, 0, 0];
        let tail = bytes.split_off(33);
        for _ in 0..1000 {
            bytes.extend(text);
        }
        bytes.extend(tail);
        let header = Header::read(&bytes).expect("a PNG header");
        assert_eq!(header.texts, 1000 * PNG_TEXT_CHUNK_BYTES);
        assert!(header.peak(bytes.len()) > 1000 * PNG_TEXT_CHUNK_BYTES);
    }

    /// A PNG wider than the plugin's exports, or whose chunk runs past its end,
    /// is not decoded.
    #[test]
    fn a_png_too_wide_or_cut_short_is_refused() {
        let error = Header::read(&STANDARD.decode(png_header(8_193, 1)).expect("base64"))
            .err()
            .expect("too wide");
        assert!(
            error.message.contains("wider than 8192"),
            "{}",
            error.message
        );
        let mut cut = STANDARD
            .decode(png(1, 1, vec![0, 0, 0, 0]))
            .expect("base64");
        cut.truncate(33);
        cut.extend([0xFF, 0xFF, 0xFF, 0xFF, b'I', b'D', b'A', b'T']);
        let error = Header::read(&cut).err().expect("cut short");
        assert!(
            error.message.contains("runs past its end"),
            "{}",
            error.message
        );
    }

    /// A second SOI, or a stuffed zero, before the frame header is refused: its
    /// decoder reads those differently and could skip the frame header counted.
    #[test]
    fn a_marker_standing_alone_before_the_jpeg_frame_is_refused() {
        let jpeg = [
            0xFF, 0xD8, 0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x0B, 8, 0, 1, 0, 1, 1, 1, 0x11, 0, 0xFF,
            0xDA,
        ];
        assert_eq!(jpeg_frame(&jpeg), None);
        assert_eq!(jpeg_frame(&[0xFF, 0xD8, 0xFF, 0x00, 0x00, 0x13]), None);
        assert_eq!(
            jpeg_frame(&[&jpeg[..2], &jpeg[4..]].concat()),
            Some((1, 1, 2))
        );
    }

    /// A JPEG fill composes, counted with eight copies of its bytes - here
    /// mostly a large APP0 segment its decoder copies with the rest. A WebP
    /// fill is refused, so the canvas picture is used.
    #[test]
    fn a_jpeg_fill_composes_and_a_webp_fill_is_refused() {
        let mut encoded = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut encoded)
            .encode(
                &[255, 0, 0].repeat(16),
                4,
                4,
                image::ExtendedColorType::Rgb8,
            )
            .expect("a JPEG encodes");
        let mut jpeg = encoded[..2].to_vec();
        for _ in 0..16 {
            jpeg.extend([0xFF, 0xE0, 0xFF, 0xFF]);
            jpeg.extend([0; 0xFFFD]);
        }
        jpeg.extend(&encoded[2..]);
        let header = Header::read(&jpeg).expect("a JPEG header");
        assert!(header.peak(jpeg.len()) >= 8 * jpeg.len() as u64);
        let mut webp = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut webp)
            .encode(
                &[0, 0, 255, 255].repeat(16),
                4,
                4,
                image::ExtendedColorType::Rgba8,
            )
            .expect("a WebP encodes");
        let scene = |bytes: &[u8]| {
            json!({
                "area": {"x": 0, "y": 0}, "width": 4, "height": 4, "page": [0, 0, 0],
                "layers": [{"transform": [[1, 0, 0], [0, 1, 0]], "width": 4, "height": 4,
                            "fills": [{"type": "IMAGE", "imageHash": "fill", "scaleMode": "FILL"}]}],
                "node": {"picture": {"data": png(1, 1, vec![0, 0, 0, 0]), "area": {"x": 0, "y": 0}}},
                "images": {"fill": STANDARD.encode(bytes)}
            })
        };
        let rgba = decode(&compose(&scene(&jpeg)).expect("composes"));
        assert!(rgba[0] > 200, "{rgba:?}");
        let error = compose(&scene(&webp)).expect_err("the WebP is refused");
        assert!(
            error.message.contains("not a PNG, JPEG or GIF"),
            "{}",
            error.message
        );
    }

    /// An image the plugin sent decoded and smaller is laid out at the original's
    /// size: a 4-wide tile of red and blue sent 2 wide still repeats every 4.
    #[test]
    fn a_decoded_image_is_laid_out_at_the_originals_size() {
        let tile = |images: Value| {
            json!({
                "area": {"x": 0, "y": 0}, "width": 8, "height": 2, "page": [0, 0, 0],
                "layers": [{"transform": [[1, 0, 0], [0, 1, 0]], "width": 8, "height": 2,
                            "fills": [{"type": "IMAGE", "imageHash": "t", "scaleMode": "TILE", "scalingFactor": 1}]}],
                "node": {"picture": {"data": png(1, 1, vec![0, 0, 0, 0]), "area": {"x": 0, "y": 0}}},
                "images": {"t": images}
            })
        };
        let (red, blue) = ([255, 0, 0, 255], [0, 0, 255, 255]);
        let original = png(4, 2, [red, red, blue, blue].concat().repeat(2));
        let smaller = png(2, 1, [red, blue].concat());
        let full = decode(&compose(&tile(json!(original))).expect("composes"));
        let laid_out = decode(
            &compose(&tile(json!({"data": smaller, "width": 4, "height": 2}))).expect("composes"),
        );
        let columns = |rgba: &[u8]| {
            rgba[..32]
                .chunks(4)
                .map(|px| px[0] > 128)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            columns(&full),
            [true, true, false, false, true, true, false, false]
        );
        assert_eq!(columns(&laid_out), columns(&full));
    }

    /// An ancestor's stroke goes over everything it holds: one pixel outside a
    /// white 2x2 square is black all round, mitred at the corners, and the
    /// node inside does not cover it.
    #[test]
    fn an_ancestors_stroke_is_drawn_round_it() {
        let scene = |align: &str, weight: f64, at: f64, size: f64| {
            json!({
                "area": {"x": 0, "y": 0}, "width": 4, "height": 4, "page": [128, 128, 128],
                "layers": [{"transform": [[1, 0, at], [0, 1, at]], "width": size, "height": size,
                            "fills": [{"type": "SOLID", "color": {"r": 1, "g": 1, "b": 1}}],
                            "strokes": [{"type": "SOLID", "color": {"r": 0, "g": 0, "b": 0}}],
                            "strokeAlign": align, "strokeWeights": [weight, weight, weight, weight]}],
                "node": {"picture": {"data": png(4, 4, [0, 255, 0, 255].repeat(16)), "area": {"x": 0, "y": 0}}}
            })
        };
        let pixel =
            |rgba: &[u8], x: usize, y: usize| rgba[(y * 4 + x) * 4..(y * 4 + x) * 4 + 3].to_vec();
        let outside = decode(&compose(&scene("OUTSIDE", 1.0, 1.0, 2.0)).expect("composes"));
        for (x, y) in [(0, 0), (1, 0), (3, 0), (0, 2), (3, 3)] {
            assert_eq!(pixel(&outside, x, y), [0, 0, 0], "({x}, {y})");
        }
        assert_eq!(pixel(&outside, 1, 1), [0, 255, 0]);
        let inside = decode(&compose(&scene("INSIDE", 1.0, 0.0, 4.0)).expect("composes"));
        assert_eq!(pixel(&inside, 0, 0), [0, 0, 0]);
        assert_eq!(pixel(&inside, 3, 2), [0, 0, 0]);
        assert_eq!(pixel(&inside, 2, 2), [0, 255, 0]);
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
