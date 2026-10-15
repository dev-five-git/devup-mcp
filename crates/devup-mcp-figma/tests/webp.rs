use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp_figma::{AssetFormat, AssetRenderMode, AssetRequest, exported_asset_from_bytes};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use sha2::{Digest, Sha256};
use std::io::Cursor;

fn request(quality: u8, lossless: bool) -> AssetRequest {
    AssetRequest {
        usage_aspect_ratio: None,
        max_width: None,
        srcset: false,
        visible_scope_node_id: None,
        asset_id: "image:fills:0".into(),
        node_id: "image".into(),
        field: "fills/0".into(),
        image_hash: Some("source".into()),
        format: AssetFormat::Webp,
        scale: 2.0,
        render_mode: AssetRenderMode::Node,
        quality: Some(quality),
        lossless: Some(lossless),
    }
}
fn pixels(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            (x.wrapping_mul(73) ^ y.wrapping_mul(19)) as u8,
            (x.wrapping_mul(11) ^ y.wrapping_mul(67)) as u8,
            (x.wrapping_mul(29) ^ y.wrapping_mul(31)) as u8,
            ((x + y) % 256) as u8,
        ])
    })
}
fn encode(image: RgbaImage, format: ImageFormat) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut out, format)
        .unwrap();
    out.into_inner()
}
#[test]
fn lossless_preserves_every_rgba_pixel_including_transparent_rgb_and_actual_identity() {
    let mut rgba = pixels(93, 61);
    rgba.put_pixel(0, 0, Rgba([91, 203, 27, 0]));
    rgba.put_pixel(1, 0, Rgba([13, 117, 201, 127]));
    let png = encode(rgba.clone(), ImageFormat::Png);
    for quality in [0, 80, 100] {
        let entry = exported_asset_from_bytes(&request(quality, true), &png).unwrap();
        let encoded = STANDARD.decode(entry.data_base64.unwrap()).unwrap();
        assert_eq!(image::guess_format(&encoded).unwrap(), ImageFormat::WebP);
        assert_eq!(image::load_from_memory(&encoded).unwrap().to_rgba8(), rgba);
        assert_eq!(entry.byte_length, Some(encoded.len()));
        assert_eq!(entry.mime_type.as_deref(), Some("image/webp"));
        let hash = Sha256::digest(&encoded)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert_eq!(entry.sha256.as_deref(), Some(hash.as_str()));
        let info = entry.render_info.unwrap();
        assert_eq!(info["fidelity"], "lossless-pixels");
        assert_eq!(info["byteEvidence"]["sha256"], hash);
        assert_eq!(info["compression"]["referencePNG"]["byteLength"], png.len());
        assert_eq!(info["outputSize"]["width"], 93.0);
    }
}
#[test]
fn lossy_quality_changes_rgb_and_bytes_but_preserves_alpha_and_uses_honest_savings() {
    let rgba = pixels(128, 96);
    let png = encode(rgba.clone(), ImageFormat::Png);
    let mut outputs = Vec::new();
    for quality in [0, 45, 100] {
        let entry = exported_asset_from_bytes(&request(quality, false), &png).unwrap();
        let bytes = STANDARD.decode(entry.data_base64.unwrap()).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), rgba.dimensions());
        assert!(
            decoded
                .pixels()
                .zip(rgba.pixels())
                .all(|(a, b)| a[3] == b[3])
        );
        assert_ne!(decoded, rgba);
        let info = entry.render_info.unwrap();
        assert_eq!(info["fidelity"], "approximate");
        assert_eq!(info["compression"]["quality"], quality);
        assert_eq!(
            info["compression"]["savingsBytes"],
            png.len() as i64 - bytes.len() as i64
        );
        outputs.push(bytes);
    }
    assert_ne!(outputs[0], outputs[1]);
    assert_ne!(outputs[1], outputs[2]);
}
#[test]
fn fill_conversion_is_labelled_transcoded_and_jpeg_baseline_is_real_png() {
    let source = encode(pixels(12, 8), ImageFormat::Png);
    let mut req = request(80, true);
    req.render_mode = AssetRenderMode::Fill;
    req.scale = 1.0;
    let e = exported_asset_from_bytes(&req, &source).unwrap();
    let info = e.render_info.unwrap();
    assert_eq!(info["representation"], "original-fill-transcoded");
    assert_eq!(info["renderedAs"], "transcoded-fill");
    assert_eq!(
        info["scaleSemantics"],
        "intrinsic-source-transcoded-no-resize"
    );
    assert!(
        info["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w == "original-fill-properties-unapplied")
    );
    let mut jpg = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(pixels(12, 8))
        .to_rgb8()
        .write_to(&mut jpg, ImageFormat::Jpeg)
        .unwrap();
    let e = exported_asset_from_bytes(&req, jpg.get_ref()).unwrap();
    let info = e.render_info.unwrap();
    assert_eq!(info["compression"]["input"]["mimeType"], "image/jpeg");
    assert_ne!(
        info["compression"]["referencePNG"]["byteLength"],
        jpg.get_ref().len()
    );
    assert_eq!(
        info["compression"]["referencePNG"]["provenance"],
        "same-decoded-original-fill-rendition"
    );
}
#[test]
fn malformed_oversize_wrong_codec_and_invalid_options_fail_closed() {
    let req = request(80, false);
    let truncated_end = encode(pixels(2, 2), ImageFormat::Png);
    assert!(exported_asset_from_bytes(&req, &truncated_end[..truncated_end.len() - 12]).is_err());
    for bytes in [
        b"not a PNG".to_vec(),
        encode(pixels(8193, 1), ImageFormat::Png),
        encode(pixels(2, 2), ImageFormat::Png)[..30].to_vec(),
    ] {
        assert!(exported_asset_from_bytes(&req, &bytes).is_err());
    }
    let mut invalid = request(101, false);
    assert!(exported_asset_from_bytes(&invalid, &encode(pixels(2, 2), ImageFormat::Png)).is_err());
    invalid.format = AssetFormat::Png;
    invalid.quality = Some(80);
    assert!(exported_asset_from_bytes(&invalid, &encode(pixels(2, 2), ImageFormat::Png)).is_err());
}

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut chunk = Vec::new();
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    chunk.extend_from_slice(kind);
    chunk.extend_from_slice(data);
    let mut crc = 0xffff_ffffu32;
    for byte in &chunk[4..] {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0xedb8_8320 } else { 0 };
        }
    }
    chunk.extend_from_slice(&(!crc).to_be_bytes());
    chunk
}

#[test]
fn pixel_depth_animation_and_compressed_metadata_limits_fail_before_encoding() {
    use std::io::Write;
    let req = request(80, true);
    let over_pixels = encode(image::RgbaImage::new(4097, 4097), ImageFormat::Png);
    assert!(
        exported_asset_from_bytes(&req, &over_pixels)
            .unwrap_err()
            .message
            .contains("16777216")
    );
    let mut deep = Cursor::new(Vec::new());
    DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(
        2,
        2,
        image::Rgba([1234u16, 2300, 65000, 32000]),
    ))
    .write_to(&mut deep, ImageFormat::Png)
    .unwrap();
    assert!(
        exported_asset_from_bytes(&req, deep.get_ref())
            .unwrap_err()
            .message
            .contains("8-bit")
    );
    let mut animated = encode(pixels(2, 2), ImageFormat::Png);
    animated.splice(33..33, png_chunk(b"acTL", &[0, 0, 0, 1, 0, 0, 0, 0]));
    assert!(
        exported_asset_from_bytes(&req, &animated)
            .unwrap_err()
            .message
            .contains("Animated PNG")
    );
    let mut zlib = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    for _ in 0..(70 * 1024) {
        zlib.write_all(&[b'x'; 1024]).unwrap();
    }
    let mut data = b"profile\0\0".to_vec();
    data.extend(zlib.finish().unwrap());
    let mut metadata = encode(pixels(2, 2), ImageFormat::Png);
    metadata.splice(33..33, png_chunk(b"iCCP", &data));
    assert!(metadata.len() < 1024 * 1024);
    assert!(
        exported_asset_from_bytes(&req, &metadata).is_err(),
        "configured limits must apply before ancillary metadata decoding"
    );
}

#[test]
fn valid_compressed_metadata_and_itxt_are_accepted_but_aggregate_inflation_is_bounded() {
    use std::io::Write;
    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        z.write_all(data).unwrap();
        z.finish().unwrap()
    }
    let source = encode(pixels(8, 8), ImageFormat::Png);
    for (kind, prefix) in [
        (b"zTXt", b"comment\0\0".as_slice()),
        (b"iCCP", b"profile\0\0".as_slice()),
        (b"iTXt", b"comment\0\x01\0en\0translated\0".as_slice()),
    ] {
        let mut data = prefix.to_vec();
        data.extend(zlib(b"small synthetic metadata"));
        let mut png = source.clone();
        png.splice(33..33, png_chunk(kind, &data));
        assert!(exported_asset_from_bytes(&request(80, true), &png).is_ok());
    }
    let mut text = source.clone();
    text.splice(
        33..33,
        png_chunk(b"iTXt", b"comment\0\0\0en\0translated\0plain text"),
    );
    assert!(exported_asset_from_bytes(&request(80, true), &text).is_ok());
    let mut data = b"comment\0\0".to_vec();
    data.extend(zlib(&vec![b'x'; 600 * 1024]));
    let chunk = png_chunk(b"zTXt", &data);
    let mut aggregate = source.clone();
    aggregate.splice(33..33, chunk.iter().chain(&chunk).copied());
    assert!(
        exported_asset_from_bytes(&request(80, true), &aggregate)
            .unwrap_err()
            .message
            .contains("metadata budget")
    );
    for malformed in [
        b"missing-null".as_slice(),
        b"comment\0\x02broken".as_slice(),
        b"comment\0\0bad-zlib".as_slice(),
    ] {
        let mut png = source.clone();
        png.splice(33..33, png_chunk(b"zTXt", malformed));
        assert!(exported_asset_from_bytes(&request(80, true), &png).is_err());
    }
}

#[test]
fn wrong_native_codec_and_unsupported_fill_codecs_are_actionable_refusals() {
    let mut jpg = Cursor::new(Vec::new());
    DynamicImage::new_rgb8(3, 2)
        .write_to(&mut jpg, ImageFormat::Jpeg)
        .unwrap();
    assert!(exported_asset_from_bytes(&request(80, false), jpg.get_ref()).is_err());
    let mut req = request(80, false);
    req.render_mode = AssetRenderMode::Fill;
    req.scale = 1.0;
    for format in [ImageFormat::Gif, ImageFormat::WebP] {
        let source = encode(pixels(3, 2), format);
        assert!(
            exported_asset_from_bytes(&req, &source)
                .unwrap_err()
                .message
                .contains("renderMode node")
        );
    }
}
#[test]
fn negative_savings_are_reported_without_clamping() {
    let mut negative = false;
    for size in [1, 2, 3, 4, 8, 16, 32] {
        let png = encode(pixels(size, size), ImageFormat::Png);
        let entry = exported_asset_from_bytes(&request(100, false), &png).unwrap();
        let info = entry.render_info.unwrap();
        let savings = info["compression"]["savingsBytes"].as_i64().unwrap();
        assert_eq!(
            savings,
            png.len() as i64 - entry.byte_length.unwrap() as i64
        );
        if savings < 0 {
            negative = true;
            assert!(info["compression"]["savingsPercent"].as_f64().unwrap() < 0.0);
        }
    }
    assert!(negative, "at least one actual encoded rendition must grow");
}
