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

When a Devup Bridge plugin serves the file, the plugin draws the reference instead, and it never refuses. `get_screenshot` draws the node alone - no layer beside, above or below it - over the fills of the frames and sections it sits in and the page background, across the node's render bounds: its effects and overflowing children included, cut by clipping ancestors (measured on fourteen nodes, among them a banner with the page header above it, a transparent header with the banner below it, a card with a drop shadow, a text trimmed to its glyphs and a photo cut by the frame it sits in). A plugin cannot ask for that picture, so it draws the closest one, always over the same bounds (`useAbsoluteBounds: false`), PNG at scale 1 with the longer edge held to 8192px as `maxDimension` holds it, sRGB. While the ancestors make one plain colour under the node - solid fills covering it, on opaque unblended layers with no stroke, rounded corner, inner effect or mask reaching it - it exports the node alone (`contentsOnly: true`) and names that colour: the same picture. Otherwise, as inside a frame painted with an image or a gradient, it exports the area as the canvas shows it (`contentsOnly: false`) and names the page background, the one thing that export leaves out. Other layers reaching into that area are in this picture and not in `get_screenshot`'s, so the plugin names them and the reason, and the export answer carries that as `referencePngNote` beside `referencePng` (resource delivery keeps it). The bridge flattens the export onto the colour named and hands it back as the same single image block, so both paths pass the same validation. A plugin built before this answers that it does not know the read; the read then goes to `get_screenshot` when the plugin reported the file's key and the direct path is logged in.

The two paths agree in size, not in every pixel. On the twelve nodes the plugin drew alone (screens 360 to 1920 wide and up to 7240 tall, a translucent popup, a 64px icon on a transparent background, a photo grid, and the nested nodes above), the photo grid differed by at most 37/255 (0.07% of its pixels by more than 16) and every other node by at most 16/255; most differences were 1/255. The two it drew as the canvas shows them (inside a frame painted with an image, the page header's shadow reaching in) differed by at most 15/255 and 9/255. `devup-mcp-visual compare` at its defaults (no channel tolerance, 0.5% threshold) therefore reports a mismatch between them, and at `--channel-tolerance 16` none except 0.07% of the grid. The render harness scored every screen within 0.02 points of the same with either reference. Keep every reference of a baseline from the same path all the same: the bridge answered every read, the PNG included, when `source.bridgeReads` equals `collection.figmaToolCalls`.

Earlier plugin builds drew something else: one returned the node's own alpha (on a translucent popup frame 10% of pixels matched `get_screenshot` and the harness scored 90.15% instead of 0.84%), the next drew every layer overlapping the node (the page header appeared on a banner it covers). Update the plugin.

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
