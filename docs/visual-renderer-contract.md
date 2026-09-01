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

The collector makes one `get_screenshot` call for the linked node, accepts only a bounded `image/png`, validates its signature and SHA-256, and records `capabilities.referencePng=true` on the artifact. `referencePng` currently applies to one linked node; Section multi-frame selection must acquire each frame by its canonical URL.

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
