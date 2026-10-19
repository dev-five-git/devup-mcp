# Real-render verification

Static validation and a successful screenshot capture do not establish visual
correctness. With the generated app running, execute its runtime audit, compare
the actual Chromium screenshots against the independently acquired Figma PNGs,
and inspect the diffs before calling the page done.

The existing `run.mjs` + `devup_runtime_audit ingest` + `devup_visual_compare`
workflow supports page, section and asset comparisons. A capture check's `pass`
means **captured**, not **visually matched**. Feed every comparison result back
to screen-plan status; missing, failed or inconclusive cells block completion.

For a command-line page gate (including CI):

```sh
npm ci
npx playwright install chromium
cargo build -p devup-mcp-visual
node verify-render.mjs --plan plan.json --references references.json --out out/page --visual-bin ../../target/debug/devup-mcp-visual
```

On Windows append `.exe` to the comparator path. With a custom Cargo target
directory use the binary under `cargo metadata`'s `target_directory`.

Use an audit plan containing the drawn viewports at deviceScaleFactor 1, with
`page-health`, `image-loaded` and the relevant geometry checks. Use the normal
runtime audit separately for widths without Figma originals. Each planned
screen/viewport must have exactly one entry in `references.json`:

```json
{
  "references": [{
    "screen": "home",
    "width": 1440,
    "height": 900,
    "path": "figma/home-1440.png",
    "sha256": "<SHA-256 of the independently exported Figma PNG>",
    "threshold": 0.03
  }]
}
```

Paths are relative to the manifest. Export the original frame at scale 1 and
the drawn dimensions, without resizing it. Never create a reference from the
generated page, replace an original to pass a test, or raise thresholds to hide
wrong backgrounds, colors or assets. Pin the original hash and retain its Figma
file/node/version provenance alongside the manifest.

`verify-render.mjs` launches the runtime-audit Chromium runner and compares every
page screenshot with `devup-mcp-visual`. It writes the runtime report, screenshots,
`real-render.json` and `visual-diff-*.png`. Exit codes: 0 means runtime and pixels
passed; 1 means a runtime or visual failure; 2 means tooling, input or reference
failure. Missing/duplicate references, changed reference bytes, missing captures,
browser errors and invalid image dimensions cannot pass. Do not infer success
from `report.status: completed`, which describes execution, not correctness.

## Offline regression

`npm test` runs all tests, including `test/real-render.test.mjs`. This needs Rust,
Node and Chromium; it does not silently skip when they are missing. The test
replays `fixtures/render-snapshot.json` through production Rust codegen, builds
the result with the real DevupUI plugin, serves the production app, and compares
the Chromium capture through the production Rust pixel comparator. The correct
page must match exactly. Removing its background, changing a card color or
requesting an unavailable asset must fail while the geometry still passes.

The small checked-in PNG oracle is **synthetic**, not a live Figma acquisition:
its independent pixel construction represents the snapshot's 320x240 frame,
with `#123456` above y=120, `#abcdef` below, and a red 80x60 card at (32,32).
`render-hero.png` is the same background without the card. The oracle is never
rendered from the generated code or updated by the test. This deterministic
offline contract proves the gate detects rendering failures; verification of an
actual Figma page requires that page's exported original as described above.

Set `DEVUP_KEEP_RENDER_EVIDENCE=1` to retain regression artifacts under `out/`
for diagnosis or upload by a CI caller. Run `npm ci`,
`npx playwright install --with-deps chromium`, and `npm test` to use this suite
in Linux CI. The repository workflow is not wired to this browser suite yet:
the GitHub proxy rejected pushes containing that workflow-file modification.
