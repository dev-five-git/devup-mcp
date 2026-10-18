
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
    // A drop shadow is cast from what lies its offset and spread away, and
    // past the margin nothing is drawn to cast it: a shadow reaching farther is
    // not composed rather than drawn smaller.
    if let Some(reach) = shadow_reach(&scene).filter(|&reach| reach > margin) {
        return Err(invalid(format!(
            "a shadow is cast from {reach} pixels away, past the {margin} drawn around the area"
        )));
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
                Some("LAYER_BLUR" | "BACKGROUND_BLUR" | "DROP_SHADOW" | "INNER_SHADOW")
            )
        })
        .map(|effect| {
            let reach = (number(&effect["radius"]) * scene.scale * 1.5)
                .ceil()
                .max(0.0) as u64;
            // A shadow's spread takes the widest (or narrowest) alpha across a
            // window twice the spread wide, once along each axis.
            let spread = if effect["type"] == "DROP_SHADOW" {
                (number(&effect["spread"]) * scene.scale).round().abs() as u64
            } else {
                0
            };
            pixels
                .saturating_mul(reach.saturating_mul(4).saturating_add(2))
                .saturating_add(if spread == 0 {
                    0
                } else {
                    pixels.saturating_mul(spread.saturating_mul(4).saturating_add(2))
                })
        })
        .fold(0_u64, u64::saturating_add);
    if taps > MAX_BLUR_TAPS {
        return Err(invalid(format!(
            "its blurs and shadow spreads would take {taps} kernel taps"
        )));
    }
    Ok(())
}

/// How far away, in pixels, the farthest drop shadow is cast from.
fn shadow_reach(scene: &Scene) -> Option<usize> {
    scene
        .layers
        .iter()
        .chain(scene.node.own.as_ref())
        .flat_map(|layer| layer.effects.iter().filter(|effect| visible(effect)))
        .filter(|effect| effect["type"] == "DROP_SHADOW")
        .map(|effect| shadow_cast(effect, scene.scale))
        .max()
}

/// How far away a drop shadow is cast from as `drop_shadow` draws it: its
/// larger offset and its spread, each rounded to whole pixels on its own.
fn shadow_cast(effect: &Value, scale: f64) -> usize {
    let rounded = |value: &Value| (number(value) * scale).round().abs() as usize;
    rounded(&effect["offset"]["x"])
        .max(rounded(&effect["offset"]["y"]))
        .saturating_add(rounded(&effect["spread"]))
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
            let shift = if matches!(
                effect["type"].as_str(),
                Some("DROP_SHADOW" | "INNER_SHADOW")
            ) {
                (number(&effect["offset"]["x"])
                    .abs()
                    .max(number(&effect["offset"]["y"]).abs())
                    + number(&effect["spread"]).abs())
                    * scene.scale
            } else {
                0.0
            };
            let reach = (sigma * 3.0 + shift).ceil() as usize;
            // Rounded apart, a drop shadow's offset and spread can be drawn a
            // pixel farther than their sum rounded up.
            if effect["type"] == "DROP_SHADOW" {
                reach.max(
                    shadow_cast(effect, scene.scale).saturating_add((sigma * 3.0).ceil() as usize),
                )
            } else {
                reach
            }
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
        for effect in layer
            .effects
            .iter()
            .filter(|effect| visible(effect) && effect["type"] == "INNER_SHADOW")
        {
            inner_shadow(&mut top.pixels, &top.clip, &shape, &geometry, &grid, effect)?;
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
        let shadows: Vec<&Value> = layer
            .effects
            .iter()
            .filter(|effect| visible(effect) && effect["type"] == "DROP_SHADOW")
            .collect();
        if !shadows.is_empty() {
            // Each shadow is cast by the layer alone, and all lie under it.
            // Where it is not shown behind the layer it is cut away under the
            // layer: under its shape when it is painted, and under what it
            // holds wherever that reaches.
            let geometry = Geometry::new(layer)?;
            let painted = layer.fills.iter().any(visible);
            let shape: Vec<f32> = (0..count)
                .map(|index| {
                    let held = pixels[index][3];
                    if painted {
                        geometry.coverage(grid.point(index), grid.scale).max(held)
                    } else {
                        held
                    }
                })
                .collect();
            let mut under = vec![[0.0; 4]; count];
            for effect in shadows {
                let shadow = drop_shadow(&pixels, &grid, effect, &shape);
                for (px, cast) in under.iter_mut().zip(shadow) {
                    composite(px, cast, Blend::Normal, 1.0);
                }
            }
            for (px, &own) in under.iter_mut().zip(&pixels) {
                composite(px, own, Blend::Normal, 1.0);
            }
            pixels = under;
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