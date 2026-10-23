//! Bounded, portable libwebp postprocessing. Figma itself exports PNG.
use std::io::{Cursor, Read};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::{ImageDecoder, ImageFormat, ImageReader, Limits};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    AssetFormat, AssetManifestEntry, AssetRequest, DevupError, ErrorCode, MAX_ASSET_BYTES,
};

// Bound work before decoding/encoding. One encoder runs per collector result;
// libwebp uses one thread, method 3, one pass, with no unbounded search settings.
pub const MAX_WEBP_DIMENSION: u32 = 8192;
pub const MAX_WEBP_PIXELS: u64 = 16_777_216;
const MAX_DECODE_BYTES: u64 = 128 * 1024 * 1024;

fn invalid(message: &str) -> DevupError {
    DevupError::new(ErrorCode::DevupInvalidInput, message, false)
}

pub fn validate_webp_options(
    format: AssetFormat,
    quality: Option<u8>,
    lossless: Option<bool>,
) -> Result<(), DevupError> {
    if quality.is_some_and(|q| q > 100) {
        return Err(invalid("WebP quality must be an integer from 0 to 100."));
    }
    if format != AssetFormat::Webp && (quality.is_some() || lossless.is_some()) {
        return Err(invalid(
            "quality and lossless apply only to format webp; remove them or request webp.",
        ));
    }
    Ok(())
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// Ancillary decompression is separate from pixel allocation. PNG decoders may
// ignore malformed profiles; conversion instead refuses malformed/unbounded
// compressed metadata before decoding any header or pixels.
pub(crate) fn validate_png_input(source: &[u8]) -> Result<(), DevupError> {
    if !source.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(invalid("PNG signature is malformed."));
    }
    let mut offset = 8usize;
    let mut metadata_bytes = 0usize;
    let mut ended = false;
    while offset
        .checked_add(12)
        .is_some_and(|end| end <= source.len())
    {
        let length = u32::from_be_bytes(source[offset..offset + 4].try_into().unwrap()) as usize;
        let end = offset
            .checked_add(12)
            .and_then(|v| v.checked_add(length))
            .ok_or_else(|| invalid("PNG chunk length overflow."))?;
        if end > source.len() {
            return Err(invalid("PNG chunk is truncated."));
        }
        let kind = &source[offset + 4..offset + 8];
        let data = &source[offset + 8..end - 4];
        if offset == 8 && (kind != b"IHDR" || length != 13) {
            return Err(invalid("PNG must begin with a 13-byte IHDR."));
        }
        if kind == b"IHDR" && offset != 8 {
            return Err(invalid("PNG has a duplicate IHDR."));
        }
        if kind == b"IEND" {
            if length != 0 || end != source.len() {
                return Err(invalid("PNG IEND or trailing bytes are malformed."));
            }
            ended = true;
        }
        if kind == b"acTL" {
            return Err(invalid(
                "Animated PNG fills cannot be flattened silently; use renderMode node for a static native rendition or retain the original animation.",
            ));
        }
        let compressed = if kind == b"iCCP" || kind == b"zTXt" {
            let separator = data
                .iter()
                .position(|b| *b == 0)
                .ok_or_else(|| invalid("PNG compressed metadata header is malformed."))?;
            if separator == 0 || separator > 79 || data.get(separator + 1) != Some(&0) {
                return Err(invalid("PNG compressed metadata method is unsupported."));
            }
            Some(
                data.get(separator + 2..)
                    .ok_or_else(|| invalid("PNG compressed metadata is truncated."))?,
            )
        } else if kind == b"iTXt" {
            let separator = data
                .iter()
                .position(|b| *b == 0)
                .ok_or_else(|| invalid("PNG text header is malformed."))?;
            let flag = *data
                .get(separator + 1)
                .ok_or_else(|| invalid("PNG text compression flag is missing."))?;
            if flag > 1 || data.get(separator + 2) != Some(&0) {
                return Err(invalid("PNG text compression method is unsupported."));
            }
            let mut start = separator + 3;
            for _ in 0..2 {
                start += data
                    .get(start..)
                    .and_then(|s| s.iter().position(|b| *b == 0))
                    .ok_or_else(|| invalid("PNG text language header is malformed."))?
                    + 1;
            }
            (flag == 1).then_some(&data[start..])
        } else {
            None
        };
        if let Some(compressed) = compressed {
            let remaining = (1024 * 1024usize).saturating_sub(metadata_bytes);
            let mut decoded = Vec::new();
            flate2::read::ZlibDecoder::new(compressed)
                .take((remaining + 1) as u64)
                .read_to_end(&mut decoded)
                .map_err(|_| invalid("PNG compressed metadata is malformed."))?;
            if decoded.len() > remaining {
                return Err(invalid(
                    "PNG compressed metadata exceeds the 1 MiB decoded metadata budget. Strip metadata or export a native node PNG.",
                ));
            }
            metadata_bytes += decoded.len();
        }
        offset = end;
    }
    if offset != source.len() || !ended {
        return Err(invalid("PNG has truncated or trailing chunk bytes."));
    }
    Ok(())
}

fn encode_webp(
    rgba: &image::RgbaImage,
    quality: u8,
    lossless: bool,
) -> Result<Vec<u8>, DevupError> {
    let mut config =
        webp::WebPConfig::new().map_err(|_| invalid("WebP encoder initialization failed."))?;
    config.lossless = i32::from(lossless);
    config.quality = f32::from(quality);
    config.exact = 1;
    config.alpha_quality = 100;
    config.method = 3;
    config.thread_level = 0;
    config.pass = 1;
    let encoded = webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height())
        .encode_advanced(&config)
        .map_err(|_| invalid("WebP encoding failed."))?;
    if encoded.is_empty() || encoded.len() > MAX_ASSET_BYTES {
        return Err(invalid("WebP output exceeds the byte budget."));
    }
    Ok(encoded.to_vec())
}

fn resized(source: &image::RgbaImage, width: u32) -> image::RgbaImage {
    let width = width.min(source.width()).max(1);
    if width == source.width() {
        return source.clone();
    }
    let height = ((f64::from(source.height()) * f64::from(width) / f64::from(source.width()))
        .round() as u32)
        .max(1);
    image::imageops::resize(source, width, height, image::imageops::FilterType::Lanczos3)
}

pub(crate) fn finish(
    request: &AssetRequest,
    mut entry: AssetManifestEntry,
) -> Result<AssetManifestEntry, DevupError> {
    validate_webp_options(request.format, request.quality, request.lossless)?;
    crate::validate_asset_options(
        request.format,
        request.render_mode,
        request.scale,
        request.max_width,
        request.srcset,
    )?;
    entry.quality = request.quality;
    entry.lossless = request.lossless;
    entry.max_width = request.max_width;
    entry.usage_aspect_ratio = request.usage_aspect_ratio;
    entry.srcset = request.srcset;
    entry.visible_scope_node_id = request.visible_scope_node_id.clone();
    if entry.status != crate::AssetStatus::Exported {
        return Ok(entry);
    }
    let original = request.render_mode.is_original();
    if request.format != AssetFormat::Webp
        && request.render_mode != crate::AssetRenderMode::OriginalSource
    {
        return Ok(entry);
    }
    let source = STANDARD
        .decode(entry.data_base64.as_deref().unwrap_or_default())
        .map_err(|_| invalid("Image input has invalid base64."))?;
    if source.is_empty() || source.len() > MAX_ASSET_BYTES {
        return Err(invalid("Image input exceeds the encoded byte budget."));
    }
    let source_format =
        image::guess_format(&source).map_err(|_| invalid("Image input codec is malformed."))?;
    let requested_format = match request.format {
        AssetFormat::Png => ImageFormat::Png,
        AssetFormat::Jpg => ImageFormat::Jpeg,
        AssetFormat::Webp => ImageFormat::WebP,
        _ => return Err(invalid("original-source requires PNG, JPG or WebP.")),
    };
    if request.format != AssetFormat::Webp && source_format == requested_format {
        // Full source identity: no decode/re-encode and no hidden upscale.
        entry.mime_type = Some(source_format.to_mime_type().into());
        return Ok(entry);
    }
    if (!original && source_format != ImageFormat::Png)
        || (original
            && !matches!(
                source_format,
                ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
            ))
        || (request.render_mode == crate::AssetRenderMode::Fill
            && source_format == ImageFormat::WebP)
    {
        return Err(invalid(
            "Conversion supports static PNG/JPEG/WebP sources; retain animated originals or explicitly choose renderMode node.",
        ));
    }
    if source_format == ImageFormat::Png {
        validate_png_input(&source)?;
    }
    // WebP animation must not become a silently flattened still.
    if source_format == ImageFormat::WebP && source.windows(4).any(|w| w == b"ANIM" || w == b"ANMF")
    {
        return Err(invalid(
            "Animated WebP conversion is unsupported; retain the original animation.",
        ));
    }
    let mut reader = ImageReader::with_format(Cursor::new(&source), source_format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_WEBP_DIMENSION);
    limits.max_image_height = Some(MAX_WEBP_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| invalid("Image input dimensions are malformed."))?;
    let (width, height) = decoder.dimensions();
    if width == 0
        || height == 0
        || width > MAX_WEBP_DIMENSION
        || height > MAX_WEBP_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_WEBP_PIXELS
    {
        return Err(invalid(
            "Image conversion exceeds 8192 pixels per side or 16777216 pixels total; use maxWidth after acquiring a supported source, select a smaller node or retain the original bytes.",
        ));
    }
    let orientation = decoder
        .orientation()
        .map_err(|_| invalid("Source orientation metadata could not be interpreted."))?;
    let encoded_dimensions = (width, height);
    let mut decoded = image::DynamicImage::from_decoder(decoder)
        .map_err(|_| invalid("Image could not be decoded within the memory budget."))?;
    if decoded.color().bits_per_pixel() / u16::from(decoded.color().channel_count()) > 8 {
        return Err(invalid(
            "Conversion supports 8-bit channels; retain higher bit-depth originals without conversion.",
        ));
    }
    decoded.apply_orientation(orientation);
    let (width, height) = (decoded.width(), decoded.height());
    let source_rgba = decoded.to_rgba8();
    let mut info = entry.render_info.take().unwrap_or_else(|| json!({}));
    info["encodedSourcePixelSize"] =
        json!({"width":encoded_dimensions.0,"height":encoded_dimensions.1});
    info["orientation"] = json!({"exif":orientation.to_exif(),"appliedToPixels":orientation!=image::metadata::Orientation::NoTransforms,"metadataPreserved":false});
    if original {
        info["sourceSize"] =
            json!({"width":width,"height":height,"aspectRatio":f64::from(width)/f64::from(height)});
        info["sourceSizeEvidence"] =
            json!("decoded-source-header-validated-against-decoded-pixels");
    }
    let display_width = info["nodeSize"]["width"]
        .as_f64()
        .filter(|w| w.is_finite() && *w > 0.0);
    if request.srcset && display_width.is_none() {
        return Err(invalid(
            "srcset requires a known positive node display width.",
        ));
    }
    let default_cap = if request.render_mode == crate::AssetRenderMode::OriginalSource
        && request.format == AssetFormat::Webp
    {
        display_width.map(|display| (display * 2.0).ceil().clamp(1.0, f64::from(width)) as u32)
    } else {
        None
    };
    let cap = request
        .max_width
        .or(default_cap)
        .unwrap_or(width)
        .min(width);
    let base_width = if request.srcset {
        (display_width.unwrap().ceil() as u32).max(1).min(cap)
    } else {
        cap
    };
    let rgba = resized(&source_rgba, base_width);
    let resized_source = rgba.width() != width || rgba.height() != height;
    let quality = request.quality.unwrap_or(85);
    let lossless = request.lossless.unwrap_or_else(|| {
        let mut colors = std::collections::BTreeSet::new();
        source_rgba.pixels().any(|pixel| pixel[3] != 255)
            && source_rgba.pixels().all(|pixel| {
                colors.insert(pixel.0);
                colors.len() <= 256
            })
    });
    let encoded = if request.format == AssetFormat::Webp {
        encode_webp(&rgba, quality, lossless)?
    } else {
        let mut output = Cursor::new(Vec::new());
        let image = image::DynamicImage::ImageRgba8(rgba.clone());
        if requested_format == ImageFormat::Jpeg && rgba.pixels().any(|p| p[3] != 255) {
            return Err(invalid(
                "JPEG cannot preserve source transparency; request PNG or WebP.",
            ));
        }
        image
            .write_to(&mut output, requested_format)
            .map_err(|_| invalid("Explicit source codec conversion failed."))?;
        output.into_inner()
    };
    if encoded.is_empty() || encoded.len() > MAX_ASSET_BYTES {
        return Err(invalid("Converted image exceeds the byte budget."));
    }
    if request.srcset {
        let target = ((display_width.unwrap() * 2.0).ceil() as u32)
            .max(1)
            .min(cap);
        let twice = resized(&source_rgba, target);
        let bytes = encode_webp(&twice, quality, lossless)?;
        if twice.width() > rgba.width() {
            entry.variants.push(crate::AssetVariant {
                density: f64::from(twice.width()) / f64::from(rgba.width()),
                width: twice.width(),
                height: twice.height(),
                byte_length: bytes.len(),
                sha256: hash(&bytes),
                mime_type: "image/webp".into(),
                data_base64: Some(STANDARD.encode(&bytes)),
                path: None,
                output_path: None,
            });
        }
    }
    let reference_png = if source_format == ImageFormat::Png
        && !resized_source
        && orientation == image::metadata::Orientation::NoTransforms
    {
        source.clone()
    } else {
        let mut output = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(rgba.clone())
            .write_to(&mut output, ImageFormat::Png)
            .map_err(|_| invalid("PNG compression baseline encoding failed."))?;
        output.into_inner()
    };
    let representation = if original {
        "original-fill-transcoded"
    } else if request.render_mode == crate::AssetRenderMode::Visible {
        "native-visible-transcoded"
    } else {
        "native-node-transcoded"
    };
    info["representation"] = json!(representation);
    info["resize"] = json!({"requestedMaxWidth":request.max_width,"applied":resized_source,"upscaled":false,"input":{"width":width,"height":height},"output":{"width":rgba.width(),"height":rgba.height()},"filter":"Lanczos3"});
    info["compression"] = json!({"codec":if request.format == AssetFormat::Webp {"libwebp"}else{"explicit-source-codec-conversion"},"quality":if request.format==AssetFormat::Webp {Some(quality)}else{None},"lossless":if request.format==AssetFormat::Webp {lossless}else{request.format==AssetFormat::Png},"sourcePixelsPreserved":(lossless || request.format==AssetFormat::Png) && !resized_source,
        "pixelScope":"decoded-8-bit-RGBA; EXIF orientation applied; color-profile interpretation unverified and metadata not preserved; resized output is a derivative",
        "outputMimeType":request.format.mime_type(),
        "alphaQuality":if request.format==AssetFormat::Webp {Some(100)}else{None},
        "exactTransparentRgb":lossless || request.format==AssetFormat::Png,
        "method":if request.format==AssetFormat::Webp {Some(3)}else{None},
        "threads":if request.format==AssetFormat::Webp {Some(1)}else{None},
        "passes":if request.format==AssetFormat::Webp {Some(1)}else{None},
        "input":{"mimeType":source_format.to_mime_type(),"byteLength":source.len(),"sha256":hash(&source)},
        "referencePNG":{"byteLength":reference_png.len(),"sha256":hash(&reference_png),"provenance":if original {"same-decoded-original-fill-rendition"}else{"same-native-Figma-node-PNG-at-requested-scale"}},
        "byteLength":encoded.len(),"savingsBytes":reference_png.len() as i64-encoded.len() as i64,"savingsPercent":100.0*(1.0-encoded.len() as f64/reference_png.len() as f64),
        "sourceSavingsBytes":source.len() as i64-encoded.len() as i64,"sourceSavingsPercent":100.0*(1.0-encoded.len() as f64/source.len() as f64),
        "limits":{"maxDimension":MAX_WEBP_DIMENSION,"maxPixels":MAX_WEBP_PIXELS,"maxDecodeBytes":MAX_DECODE_BYTES,"maxDecodedCompressedMetadataBytes":1048576},"independentFigmaReference":false});
    info["transcoded"] = json!(true);
    if !info["warnings"].is_array() {
        info["warnings"] = json!([]);
    }
    info["warnings"]
        .as_array_mut()
        .unwrap()
        .push(json!("derivative-source-metadata-not-preserved"));
    info["warnings"]
        .as_array_mut()
        .unwrap()
        .push(json!("source-color-profile-interpretation-unverified"));
    if request.srcset && (f64::from(width) < display_width.unwrap() * 2.0) {
        info["warnings"]
            .as_array_mut()
            .unwrap()
            .push(json!("srcset-2x-source-resolution-insufficient"));
    }
    entry.render_info = Some(crate::asset_render_info::measured_info(
        request.render_mode,
        request.format,
        request.scale,
        Some(info),
        Some(&encoded),
    ));
    entry.mime_type = Some(requested_format.to_mime_type().into());
    entry.byte_length = Some(encoded.len());
    entry.sha256 = Some(hash(&encoded));
    entry.data_base64 = Some(STANDARD.encode(&encoded));
    if let Some(diagnostics) = &mut entry.export_diagnostics {
        diagnostics.render_info = entry.render_info.clone();
    }
    Ok(entry)
}
