

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

    /// A shadow's spread grows or shrinks it before it is blurred: a black
    /// square two pixels wide, its shadow moved four to the right and not
    /// blurred, casts four red pixels; with a spread of one, sixteen; with a
    /// spread of minus one, none.
    #[test]
    fn a_shadows_spread_grows_or_shrinks_it() {
        let scene = |spread: f64| {
            json!({
                "area": {"x": 0, "y": 0}, "width": 10, "height": 6, "page": [255, 255, 255],
                "layers": [{"transform": [[1, 0, 2], [0, 1, 2]], "width": 2, "height": 2,
                            "fills": [{"type": "SOLID", "color": {"r": 0, "g": 0, "b": 0}}],
                            "effects": [{"type": "DROP_SHADOW", "radius": 0, "spread": spread,
                                         "offset": {"x": 4, "y": 0}, "color": {"r": 1, "g": 0, "b": 0, "a": 1}}]}],
                "node": {"picture": {"data": png(1, 1, vec![0, 0, 0, 0]), "area": {"x": 0, "y": 0}}}
            })
        };
        let red = |rgba: &[u8]| {
            rgba.chunks(4)
                .filter(|px| px[0] > 200 && px[1] < 60)
                .count()
        };
        assert_eq!(red(&decode(&compose(&scene(0.0)).expect("composes"))), 4);
        assert_eq!(red(&decode(&compose(&scene(1.0)).expect("composes"))), 16);
        assert_eq!(red(&decode(&compose(&scene(-1.0)).expect("composes"))), 0);
    }

    /// A spread is drawn as asked or not at all. The work it takes is counted
    /// before drawing: unblurred, a spread of 500 over a thousand-pixel square
    /// would take eight billion comparisons. A shadow cast from farther than
    /// the margin drawn around the area - here a square 550 pixels to the left
    /// with a spread of 600, whose shadow reaches the area while one spread
    /// 512 would not - is not composed rather than drawn with less spread.
    #[test]
    fn a_large_spread_is_counted_and_never_drawn_smaller() {
        let scene = |width: u32, layer: Value| {
            json!({
                "area": {"x": 0, "y": 0}, "width": width, "height": width, "page": [255, 255, 255],
                "layers": [layer],
                "node": {"picture": {"data": png(1, 1, vec![0, 0, 0, 0]), "area": {"x": 0, "y": 0}}}
            })
        };
        let shadowed = |x: f64, size: f64, spread: f64| {
            json!({"transform": [[1, 0, x], [0, 1, 0]], "width": size, "height": size,
                   "fills": [{"type": "SOLID", "color": {"r": 0, "g": 0, "b": 0}}],
                   "effects": [{"type": "DROP_SHADOW", "radius": 0, "spread": spread,
                                "offset": {"x": 0, "y": 0}, "color": {"r": 1, "g": 0, "b": 0, "a": 1}}]})
        };
        let error = compose(&scene(1000, shadowed(0.0, 1000.0, 500.0))).expect_err("too much work");
        assert!(error.message.contains("kernel taps"), "{}", error.message);
        let error =
            compose(&scene(4, shadowed(-552.0, 2.0, 600.0))).expect_err("cast from too far");
        assert!(error.message.contains("pixels away"), "{}", error.message);
    }

    /// A shadow's reach is counted as `drop_shadow` draws it, its offset and
    /// spread each rounded: offset 255.5 and spread 256.5 come to 512 together
    /// but are drawn 256 and 257 pixels away, one past the 512 drawn around
    /// the area, so they are not composed. Offset 0.5 and spread 0.5 are drawn
    /// as offset 1 and spread 1 are, and the margin is drawn wide enough for
    /// them.
    #[test]
    fn a_shadows_reach_is_counted_as_it_is_drawn() {
        let scene = |offset: f64, spread: f64| {
            json!({
                "area": {"x": 0, "y": 0}, "width": 4, "height": 4, "page": [255, 255, 255],
                "layers": [{"transform": [[1, 0, -2], [0, 1, 1]], "width": 2, "height": 2,
                            "fills": [{"type": "SOLID", "color": {"r": 0, "g": 0, "b": 0}}],
                            "effects": [{"type": "DROP_SHADOW", "radius": 0, "spread": spread,
                                         "offset": {"x": offset, "y": 0}, "color": {"r": 1, "g": 0, "b": 0, "a": 1}}]}],
                "node": {"picture": {"data": png(1, 1, vec![0, 0, 0, 0]), "area": {"x": 0, "y": 0}}}
            })
        };
        let error = compose(&scene(255.5, 256.5)).expect_err("cast from past the margin");
        assert!(
            error.message.contains("513 pixels away"),
            "{}",
            error.message
        );
        assert_eq!(
            compose(&scene(0.5, 0.5)).expect("composes"),
            compose(&scene(1.0, 1.0)).expect("composes")
        );
    }

    /// An inner shadow falls inside the layer from the side its offset points
    /// away from: moved two down and not blurred, it darkens the top two rows of
    /// a white square, and nothing below them or outside the square.
    #[test]
    fn an_inner_shadow_falls_inside_the_layer() {
        let scene = json!({
            "area": {"x": 0, "y": 0}, "width": 6, "height": 6, "page": [128, 128, 128],
            "layers": [{"transform": [[1, 0, 1], [0, 1, 1]], "width": 4, "height": 4,
                        "fills": [{"type": "SOLID", "color": {"r": 1, "g": 1, "b": 1}}],
                        "effects": [{"type": "INNER_SHADOW", "radius": 0, "spread": 0,
                                     "offset": {"x": 0, "y": 2}, "color": {"r": 0, "g": 0, "b": 0, "a": 1}}]}],
            "node": {"picture": {"data": png(1, 1, vec![0, 0, 0, 0]), "area": {"x": 0, "y": 0}}}
        });
        let rgba = decode(&compose(&scene).expect("composes"));
        let pixel = |x: usize, y: usize| rgba[(y * 6 + x) * 4];
        assert_eq!(
            [pixel(2, 1), pixel(2, 2), pixel(2, 3), pixel(2, 4)],
            [0, 0, 255, 255]
        );
        assert_eq!(pixel(0, 1), 128);
    }

    /// A shadow not shown behind its layer is cut away under a painted layer's
    /// whole shape, but under an unpainted one (a group) only where it holds
    /// something: the shadow of a translucent square stays out of its box,
    /// while a group's shadow shows inside its box beside what it holds.
    #[test]
    fn a_shadow_is_cut_away_under_its_layer_only() {
        let scene = |fills: Value| {
            json!({
                "area": {"x": 0, "y": 0}, "width": 6, "height": 4, "page": [255, 255, 255],
                "layers": [{"transform": [[1, 0, 1], [0, 1, 1]], "width": 4, "height": 2, "fills": fills,
                            "effects": [{"type": "DROP_SHADOW", "radius": 0, "spread": 0, "showShadowBehindNode": false,
                                         "offset": {"x": 1, "y": 0}, "color": {"r": 1, "g": 0, "b": 0, "a": 1}}]}],
                "node": {"picture": {"data": png(1, 2, vec![0, 0, 255, 255, 0, 0, 255, 255]), "area": {"x": 1, "y": 1}}}
            })
        };
        let pixel =
            |rgba: &[u8], x: usize, y: usize| rgba[(y * 6 + x) * 4..(y * 6 + x) * 4 + 3].to_vec();
        let painted = decode(
            &compose(&scene(
                json!([{"type": "SOLID", "opacity": 0.5, "color": {"r": 0, "g": 1, "b": 0}}]),
            ))
            .expect("composes"),
        );
        // Cast by a half-transparent fill, the shadow is half as strong.
        assert_eq!(
            pixel(&painted, 5, 1),
            [255, 128, 128],
            "the shadow past the square shows"
        );
        assert!(
            pixel(&painted, 3, 1)[0] < 200,
            "no shadow inside the painted square"
        );
        let group = decode(&compose(&scene(json!([]))).expect("composes"));
        assert_eq!(
            pixel(&group, 3, 1),
            [255, 255, 255],
            "nothing is cast where the group holds nothing"
        );
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
