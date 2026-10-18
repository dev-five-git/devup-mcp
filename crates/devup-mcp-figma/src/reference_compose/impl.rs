

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

/// A drop shadow the layer casts: its alpha, moved, spread, blurred and
/// coloured; unless shown behind it, cut away by `shape`.
fn drop_shadow(pixels: &[Px], grid: &Grid, effect: &Value, shape: &[f32]) -> Vec<Px> {
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
    let spread = (number(&effect["spread"]) * grid.scale).round() as isize;
    if spread != 0 {
        spread_alpha(&mut shadow, width, height, spread);
    }
    let mut shadow = gaussian(
        &shadow,
        width,
        height,
        number(&effect["radius"]) * grid.scale / 2.0,
    );
    if effect["showShadowBehindNode"] == false {
        for (px, &cover) in shadow.iter_mut().zip(shape) {
            px[3] *= 1.0 - cover;
        }
    }
    shadow
}

/// An inner shadow over a layer's paints and under what it holds: what lies
/// outside the layer, moved by the offset, grown inward by the spread, blurred
/// and coloured, inside the layer.
fn inner_shadow(
    pixels: &mut [Px],
    clip: &[f32],
    shape: &[f32],
    geometry: &Geometry,
    grid: &Grid,
    effect: &Value,
) -> Result<(), DevupError> {
    let colour = colour(&effect["color"], 1.0);
    let blend = Blend::parse(effect["blendMode"].as_str())?;
    let (dx, dy) = (
        number(&effect["offset"]["x"]),
        number(&effect["offset"]["y"]),
    );
    let spread = number(&effect["spread"]);
    let outside: Vec<Px> = (0..pixels.len())
        .map(|index| {
            let (x, y) = grid.point(index);
            let inside = geometry.grown((x - dx, y - dy), grid.scale, [-spread; 4]);
            [0.0, 0.0, 0.0, 1.0 - inside]
        })
        .collect();
    let blurred = gaussian(
        &outside,
        grid.width,
        grid.height,
        number(&effect["radius"]) * grid.scale / 2.0,
    );
    for (index, pixel) in pixels.iter_mut().enumerate() {
        let coverage = blurred[index][3] * shape[index] * clip[index];
        composite(pixel, colour, blend, coverage);
    }
    Ok(())
}

/// Grows a shadow's alpha by `by` pixels (shrinks it when negative), as a
/// shadow's spread does. Past the grid's edge the shadow is taken to go on.
fn spread_alpha(shadow: &mut [Px], width: usize, height: usize, by: isize) {
    let reach = by.unsigned_abs();
    let pick = |a: f32, b: f32| if by > 0 { a.max(b) } else { a.min(b) };
    for horizontal in [true, false] {
        let (lines, length) = if horizontal {
            (height, width)
        } else {
            (width, height)
        };
        let at = |line: usize, step: usize| {
            if horizontal {
                line * width + step
            } else {
                step * width + line
            }
        };
        let mut alphas = vec![0.0_f32; length];
        for line in 0..lines {
            for (step, alpha) in alphas.iter_mut().enumerate() {
                *alpha = shadow[at(line, step)][3];
            }
            for step in 0..length {
                let (from, to) = (step.saturating_sub(reach), (step + reach).min(length - 1));
                shadow[at(line, step)][3] =
                    alphas[from..=to].iter().copied().fold(alphas[step], pick);
            }
        }
    }
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