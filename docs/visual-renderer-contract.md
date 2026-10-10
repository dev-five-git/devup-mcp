# Visual renderer contract

`devup-mcp` keeps Figma acquisition and pixel comparison deterministic while leaving application rendering to the consuming repository. This avoids embedding a browser or JavaScript runtime in the MCP server.

## 1. Acquire the reference

Request `referencePng` together with the desired projection:

```json
{
  "url": "https://www.figma.com/design/<file>/<name>?node-id=<node>",
  "outputs": ["tsx", "sourceMap", "referencePng"],
  "delivery": "resource"
}
```

The collector makes one `get_screenshot` call for the linked node, asking for the PNG inline (`enableBase64Response`) at the node's own size (`maxDimension` raised from the tool's 1024px default to 8192px), takes the one `image/png` block beside the URL text the tool also writes, validates its signature and SHA-256, and records `capabilities.referencePng=true` on the artifact. `referencePng` currently applies to one linked node; Section multi-frame selection must acquire each frame by its canonical URL.

When a Devup Bridge plugin serves the file, the plugin draws the reference instead, and it never refuses. `get_screenshot` draws the node in its ancestors' context - their paints (gradients and images included), clipping, opacity, blend modes, layer blur and drop shadow - with every other layer left out, across the node's render bounds: its effects and overflowing children included, cut by clipping ancestors. The plugin always exports those bounds the same way (`useAbsoluteBounds: false`, PNG at scale 1 with the longer edge held to 8192px as `maxDimension` holds it, sRGB):

- No other layer draws into the area: the canvas export (`contentsOnly: false`) is that picture. The plugin names the page background, the one thing the export leaves out, and the bridge flattens the export onto it.
- Another layer does: a plugin cannot leave a layer out of an export, so it exports the node alone (`contentsOnly: true`) and describes its ancestors - transform, size and corner radii, fills, opacity and blend mode, clipping, effects, masks, and the original bytes of the images their fills use - and devup-mcp draws the rest: solid, gradient (linear, radial, angular, diamond) and image (PNG, JPEG or GIF; fill, fit, tile, crop) paints, rounded clipping, the eighteen blend modes, alpha and luminance masks, layer blur, drop shadows (spread included; one not shown behind its layer is cut away under it) and inner shadows, a rectangle-like ancestor's stroke (solid or gradient, undashed, mitred), an ancestor's background blur, and the node's own background blur (then the node is drawn from its own paints and its children's exports, because its export samples the layers behind it). A mask is exported by its box, since Figma gives it no render bounds, and a node inside a mask's own layers is not drawn: get_screenshot shows only what lies under it. An image fill devup-mcp does not decode - not a PNG, JPEG or GIF (WebP among them), longer than 8192 pixels on an edge, or mostly metadata - the plugin's UI decodes as a browser does and returns as a PNG no larger than 8192 on its longest edge, laid out at the original's size. The UI decodes one image at a time and a scene's images share 20 seconds for it; an image whose conversion would take over 512 MiB (the whole image decoded at 4 bytes a pixel and up to 8 a pixel for the PNG drawn from it), or one left when the time is up, goes as it is.
- What devup-mcp cannot draw is drawn as the canvas shows it, and the export answer carries the reason and the layers reaching in as `referencePngNote` beside `referencePng` (resource delivery keeps it). With the direct path logged in, `get_screenshot` answers instead and `source.directFallbackReasons` says `reference-png-approximate`. This covers:
  - an ancestor's stroke that is not a rectangle's, is dashed, has round or bevelled joins outside or centred, or is painted with anything but a solid colour or a gradient;
  - a background blur taking in the layers below on a layer inside the node, or on a node that cannot hold children (a rectangle, a vector) or has a stroke;
  - an image fill with rotation or filters, any other paint, an effect devup-mcp does not know (noise, texture);
  - an area over 8M pixels, or images over 64 MiB together;
  - a drop shadow cast from farther away - its offset and spread together - than the margin drawn around the area (at most 512 pixels);
  - a composition that would take over 1.5 GiB or over four billion blur and shadow-spread kernel taps, counted before it is drawn, the masks each layer keeps and the decoding of every picture and image included, counted from their headers before any decoder is made (PNG and GIF decoders are held to that allowance, and what a PNG decoder keeps outside it - the rows it unfilters, an entry for each text chunk - is counted too; a JPEG, whose decoder takes no limit, is counted with its coefficient buffers at the sampling factors its frame header names and the copies of its bytes, and one with a marker standing alone before its frame header - another SOI, a restart, a stuffed zero - is not decoded);
  - an image the plugin's UI did not hand back as a PNG (it could not decode it either, converting it would take over 512 MiB, the scene's time for conversions ran out, the plugin predates that decoding, or its check of format, size and byte count did not pick the image out) that is not a PNG, JPEG or GIF (a WebP among them: its decoder takes no limit and sizes buffers no header bounds), is a PNG wider than 8192 pixels (a wide interlaced 16-bit PNG's unfiltered rows outgrow the allowance), or is a PNG or GIF that does not decode within its allowance (a PNG with large metadata, for one); and a picture or image whose header cannot be read;
  - a part the plugin could not export, or a composition that stopped unexpectedly.

When the node is drawn from its own paints and children, its mask cuts all of them, its background blur included.

An answer over 6 MiB is held by the plugin and read in 4 MiB pieces, as ASCII so an offset means the same on both sides.
- A held answer stays until it is read or two minutes pass; another request never evicts it.
- While held answers reach 192 MiB the plugin answers `DEVUP_REFERENCE_PNG_BUSY` instead of drawing, and the bridge asks again for up to 45 seconds while the plugin goes on serving other requests' pieces. The 192 MiB is where new pictures stop being taken, not a ceiling: one answer taken below it (at most 90 MiB) can go past it.
- An answer that vanished before it was read (the plugin was run again) is fetched once more.
- An answer that would exceed 90 MiB, under the 96 MiB the bridge reads back, drops the composition and carries the canvas picture with the reason. The bridge hands the picture back as the same single image block `get_screenshot` gives, so both paths pass the same validation. A plugin built before this answers that it does not know the read; the read then goes to `get_screenshot` when the plugin reported the file's key and the direct path is logged in.

The two paths agree in size, not in every pixel: the desktop app and Figma's servers render apart. Measured on the devup Test file against `get_screenshot`: the six nodes devup-mcp composed (over gradients, under a translucent, blurred and shadowed ancestor, with their own background blur, rotated) were within 2/255 on 99.65% to 99.97% of pixels and at most 15/255, except the rim a section clips off a blurred ancestor (84/255, 0.19% of that node). Eleven harness screens (360 to 1920 wide, up to 7240 tall) were within 2/255 on at least 99.98% and at most 21/255, the photo grid 90.5% and at most 37/255 (0.07% over 16). Of twelve nodes drawn as the canvas shows them, all were within 2/255 on at least 99.09%, except one whose small blurred text the plugin's own export renders differently (94.45%, at most 67/255). `devup-mcp-visual compare` at its defaults (no channel tolerance, 0.5% threshold) therefore reports a mismatch between the paths, and at `--channel-tolerance 16` almost none. The render harness scored every screen within 0.02 points of the same with either reference. Keep every reference of a baseline from the same path all the same: the bridge answered every read, the PNG included, when `source.bridgeReads` equals `collection.figmaToolCalls`.

Earlier plugin builds drew something else: one returned the node's own alpha (on a translucent popup frame 10% of pixels matched `get_screenshot` and the harness scored 90.15% instead of 0.84%), the next drew every layer overlapping the node (the page header appeared on a banner it covers), the next drew the node alone over one colour and, where that colour could not be named, the area as the canvas shows it with other layers in it. Update the plugin.

## 2. Render the generated component

The consumer should persist a content-free run manifest next to `actual.png`:

```json
{
  "schemaVersion": 1,
  "tsxResource": { "uri": "devup://artifact/.../manifest", "sha256": "..." },
  "referenceResource": { "uri": "devup://artifact/.../manifest", "sha256": "..." },
  "viewport": { "width": 360, "height": 740, "deviceScaleFactor": 1 },
  "themePath": "devup.json",
  "assetDirectory": "public/figma-assets",
  "fontManifest": [{ "family": "Pretendard", "sha256": "..." }],
  "renderer": { "name": "playwright-chromium", "version": "..." },
  "devupUiVersion": "...",
  "actualPng": "actual.png",
  "environmentStatus": "valid"
}
```

Paths are consumer-local examples; hashes and pinned versions are required for reproducibility. A missing font/asset, renderer version mismatch, failed readiness signal, console/runtime error, or viewport mismatch MUST set `environmentStatus` to `environment-invalid` and stop before pixel metrics are treated as a product pass.

When the reference is given by `artifactId` and the manifest omits `referenceResource`, the server pins it itself: the reference is the PNG it captured and holds, so it records that PNG's SHA-256 and reports `environment.filledByServer: ["referenceResource"]`. A reference given by path is the consumer's file and is not filled. A `devup_runtime_audit` ingest hands back these calls ready to run for every screenshot taken at a drawn width (`visualComparisons`), with the fields the audit verified filled in.

The repository-owned renderer MUST:

- render the generated TSX using the repository's real DevupUI configuration, fonts, assets, and CSS reset;
- use the reference PNG width and height as the viewport and output dimensions;
- disable animations, transitions, carets, timestamps, network-dependent content, and nondeterministic data;
- wait for `document.fonts.ready`, decoded images, and the application's stable-ready signal;
- capture an opaque or transparent PNG without resizing after capture;
- keep OS, browser engine/version, device scale factor, font files, locale, and timezone pinned in CI.

The renderer writes `actual.png`. Its implementation is intentionally outside this Rust workspace because it is application-specific and may use Playwright, a browser harness, or another deterministic renderer.

The exact sequence is: request TSX/theme/reference resources, reconstruct and hash-check every resource chunk, build and type-check the generated component inside the consumer, validate the environment manifest, render at the declared viewport, write `actual.png`, and only then invoke the comparator. A pixel report from an invalid environment is diagnostic evidence, never a passing result.

## 3. Compare in pure Rust

```bash
cargo run -p devup-mcp-visual --release -- compare \
  --reference reference.png \
  --actual actual.png \
  --diff diff.png \
  --channel-tolerance 0 \
  --max-changed-ratio 0.005
```

The command decodes only PNG, normalizes both images to RGBA8, compares all four channels, emits one JSON report, and exits with code `0` for `exact` or `within-threshold`, `1` for a visual mismatch, and `2` for invalid input. Dimension differences always fail. The default changed-pixel threshold is 0.5%; teams should tighten it to zero for fully deterministic screens.

The diff PNG marks every pixel beyond `channel-tolerance` in opaque red. The JSON report contains dimensions, changed/total pixels, changed ratio, maximum channel delta, configured thresholds, and the diff path; it does not contain source pixels or Figma document content.

## Security and privacy

Reference screenshots may contain private designs or personal information. They are held in the same bounded, expiring in-memory artifact store as other collected payloads, are never logged, and are only written to disk when an explicit allowlisted `outputPaths.referencePng` is supplied. Do not commit reference, actual, or diff PNGs unless the repository explicitly treats them as approved test fixtures.
