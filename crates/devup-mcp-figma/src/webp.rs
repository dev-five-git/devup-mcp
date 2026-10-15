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
pub const MAX_WEBP_PIXELS: u64 = 4_194_304;
const MAX_DECODE_BYTES: u64 = 64 * 1024 * 1024;

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

pub(crate) fn finish(
    request: &AssetRequest,
    mut entry: AssetManifestEntry,
) -> Result<AssetManifestEntry, DevupError> {
    validate_webp_options(request.format, request.quality, request.lossless)?;
    entry.quality = request.quality;
    entry.lossless = request.lossless;
    if request.format != AssetFormat::Webp || entry.status != crate::AssetStatus::Exported {
        return Ok(entry);
    }
    let source = STANDARD
        .decode(entry.data_base64.as_deref().unwrap_or_default())
        .map_err(|_| invalid("WebP input has invalid base64."))?;
    if source.is_empty() || source.len() > MAX_ASSET_BYTES {
        return Err(invalid("WebP input exceeds the encoded byte budget."));
    }
    let source_format =
        image::guess_format(&source).map_err(|_| invalid("WebP input codec is malformed."))?;
    let original = request.render_mode == crate::AssetRenderMode::Fill;
    if (!original && source_format != ImageFormat::Png)
        || (original && !matches!(source_format, ImageFormat::Png | ImageFormat::Jpeg))
    {
        return Err(invalid(
            "WebP requires a native PNG, or an explicit fill with a PNG/JPEG source. For animated or other source codecs use renderMode node.",
        ));
    }
    if source_format == ImageFormat::Png {
        validate_png_input(&source)?;
    }
    let mut reader = ImageReader::with_format(Cursor::new(&source), source_format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_WEBP_DIMENSION);
    limits.max_image_height = Some(MAX_WEBP_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let decoder = reader
        .into_decoder()
        .map_err(|_| invalid("WebP input dimensions are malformed."))?;
    let (width, height) = decoder.dimensions();
    if width == 0
        || height == 0
        || width > MAX_WEBP_DIMENSION
        || height > MAX_WEBP_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_WEBP_PIXELS
    {
        return Err(invalid(
            "WebP input exceeds 8192 pixels per side or 4194304 pixels total. Lower scale, export a smaller node, or retain PNG.",
        ));
    }
    let decoded = image::DynamicImage::from_decoder(decoder)
        .map_err(|_| invalid("WebP input could not be decoded within the memory budget."))?;
    if decoded.color().bits_per_pixel() / u16::from(decoded.color().channel_count()) > 8 {
        return Err(invalid(
            "WebP supports 8-bit channels. Retain PNG for higher bit-depth inputs; no precision-reducing conversion was performed.",
        ));
    }
    if decoded.width() != width || decoded.height() != height {
        return Err(invalid("WebP decoded dimensions changed."));
    }
    let rgba = decoded.to_rgba8();
    let quality = request.quality.unwrap_or(80);
    let lossless = request.lossless.unwrap_or(false);
    let mut config =
        webp::WebPConfig::new().map_err(|_| invalid("WebP encoder initialization failed."))?;
    config.lossless = i32::from(lossless);
    config.quality = f32::from(quality);
    config.exact = 1;
    config.alpha_quality = 100;
    config.method = 3;
    config.thread_level = 0;
    config.pass = 1;
    let encoded = webp::Encoder::from_rgba(rgba.as_raw(), width, height)
        .encode_advanced(&config)
        .map_err(|_| invalid("WebP encoding failed."))?;
    if encoded.is_empty()
        || encoded.len() > MAX_ASSET_BYTES
        || image::guess_format(&encoded).ok() != Some(ImageFormat::WebP)
    {
        return Err(invalid(
            "WebP output exceeds the byte budget or is invalid.",
        ));
    }
    // For JPEG fills compare with an actual PNG encoding of the same decoded
    // rendition. A JPEG byte length must never be labelled referencePNG.
    let reference_png = if source_format == ImageFormat::Png {
        source.clone()
    } else {
        let mut output = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(rgba)
            .write_to(&mut output, ImageFormat::Png)
            .map_err(|_| invalid("PNG compression baseline encoding failed."))?;
        output.into_inner()
    };
    let mut info = entry.render_info.take().unwrap_or_else(|| json!({}));
    info["representation"] = json!(if original {
        "original-fill-transcoded"
    } else {
        "native-node-transcoded"
    });
    info["sourceSize"] = if original {
        json!({"width":width,"height":height,"aspectRatio":f64::from(width)/f64::from(height)})
    } else {
        info["sourceSize"].clone()
    };
    if original {
        info["sourceSizeEvidence"] =
            json!("decoded-source-header-validated-against-decoded-pixels");
    }
    info["compression"] = json!({"codec":"libwebp","quality":quality,"lossless":lossless,
        "pixelScope":"decoded-8-bit-RGBA; orientation/color-profile/other-metadata-not-preserved",
        "qualitySemantics":if lossless {"compression-effort; decoded RGBA preserved"} else {"lossy RGB quality; alpha lossless"},
        "alphaQuality":100,"exactTransparentRgb":lossless,"method":3,"threads":1,"passes":1,
        "input":{"mimeType":source_format.to_mime_type(),"byteLength":source.len(),"sha256":hash(&source)},
        "referencePNG":{"byteLength":reference_png.len(),"sha256":hash(&reference_png),
            "provenance":if original {"same-decoded-original-fill-rendition"} else {"same-native-Figma-node-PNG-at-requested-scale"}},
        "byteLength":encoded.len(),"savingsBytes":reference_png.len() as i64 - encoded.len() as i64,
        "savingsPercent":100.0*(1.0-encoded.len() as f64/reference_png.len() as f64),
        "limits":{"maxDimension":MAX_WEBP_DIMENSION,"maxPixels":MAX_WEBP_PIXELS,"maxDecodeBytes":MAX_DECODE_BYTES,"maxDecodedCompressedMetadataBytes":1048576},
        "independentFigmaReference":false});
    info["transcoded"] = json!(true);
    if !info["warnings"].is_array() {
        info["warnings"] = json!([]);
    }
    info["warnings"]
        .as_array_mut()
        .unwrap()
        .push(json!("webp-source-metadata-not-preserved"));
    entry.render_info = Some(crate::asset_render_info::measured_info(
        request.render_mode,
        request.format,
        request.scale,
        Some(info),
        Some(&encoded),
    ));
    entry.mime_type = Some("image/webp".into());
    entry.byte_length = Some(encoded.len());
    entry.sha256 = Some(hash(&encoded));
    entry.data_base64 = Some(STANDARD.encode(&*encoded));
    if let Some(diagnostics) = &mut entry.export_diagnostics {
        diagnostics.render_info = entry.render_info.clone();
    }
    Ok(entry)
}
