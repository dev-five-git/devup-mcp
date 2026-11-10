# Real-render verification

Screens may assert `expect.selected` with a semantic `group` and `target`,
`attribute` (`aria-current` or `aria-selected`), true selection `value`, and
captured `styles` such as `{ "background-color": "rgb(0, 87, 187)" }`.
The executor requires exactly one visible selected control, matching computed
paint and an optional visible `content` target. A URL or ARIA update with stale
highlight paint fails. Use source-backed expected colours, not invented ones.
Steps accept `press` (target + key value) and `back`, `forward`, `reload` (no
target/value) to verify both directions, keyboard, history and persistence.
The `markup` probe also detects a short-page footer ending above the viewport.

Generated plans include a `markup` check for document language, semantic
landmarks, one primary heading, heading levels and image `alt` attributes.
It also records controls below 24 CSS pixels for review of target-size
exceptions; size alone is not treated as a conformance verdict.

For pages without Figma attributes, declare explicit node-to-CSS mappings in
the app's `.devup/rendered-targets.json` before generating the audit plan:

```json
{ "routes": { "/": { "nodes": { "10:1": "#page", "10:12": "#hero" } } } }
```

These selectors replace attribute locators in geometry and capture checks,
and populate `completeness.nodeSelectors`. They must identify the actual
rendered target from the implementation; DOM order alone is not evidence.
Unmapped nodes remain not-run; absent or ambiguous matches cannot pass.

At drawn widths, `figma.frames[].height` records the reference frame height,
independently of the actual browser viewport (a tall frame can use a 900px window).
Capture the natural full page without forcing its height or cropping to the frame.
Record actual inner width/height, DPR, visual viewport scale and root/body CSS zoom
in `captureContext`. Different PNG dimensions make strict pixels inconclusive;
compare logical geometry and explicit completeness checks separately. A shorter
capture alone does not prove missing content. `minHeightRatio` remains an explicit
logical completeness condition; no hidden exact-frame-height override is applied.

Geometry and captures pause running animations at `currentTime = 0` and disable
CSS transitions after scroll reveals settle. An entrance on a view timeline
(`animation-timeline: view()`) is bound to scroll position, not to a clock: it
is not waited for, and it is cancelled rather than paused, so every block shows
its resting style instead of the hidden first frame a full-page capture would
otherwise catch below the fold (`frozenAnimations.scrollLinkedCount`). The runner restores motion before
interaction and view-transition checks, without changing reduced-motion media
preferences. Each screen reports `frozenAnimations` with unique target count,
selectors and animation count. Ingest exposes this as executor-declared evidence,
not independently verified browser state.

For a dev server, capture its stdout/stderr to a log and pass
`--server-log /absolute/path/server.log` to `run.mjs` (or `serverLog` to
`runPlan`). The file must already exist. Only output appended during the audit
is considered. Vite's `new dependencies optimized, reloading` and
`optimized dependencies changed. reloading` messages become
`dev-reoptimization-reload` environment errors. SSR `program reload` and browser
HMR `full-reload` messages become `dev-server-reload`; a WebSocket reload alone
does not establish that the optimizer caused it.

Affected screens are skipped with inconclusive checks and their captures are
withheld from visual comparison. The report is partial; stabilize the server
and rerun. Later independent routes still run. Console optimizer messages are
also recognized without a log, but Vite normally prints optimizer and SSR
reload diagnostics only on the server, so browser-only observation is incomplete.

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

Use an audit plan containing the drawn CSS widths and an independent window height, with
`page-health`, `image-loaded` and the relevant geometry checks. Use the normal
runtime audit separately for widths without Figma originals. Each planned
screen/viewport must have exactly one entry in `references.json`. `height` is the reference image height; optional `viewportHeight` identifies the browser window when a width is repeated:

```json
{
  "references": [{
    "screen": "home",
    "width": 1440,
    "height": 5953,
    "viewportHeight": 900,
    "path": "figma/home-1440.png",
    "sha256": "<SHA-256 of the independently exported Figma PNG>",
    "threshold": 0.03
  }]
}
```

Native 1x references and measured unit-scale captures support direct strict pixels.
Non-unit DPR or zoom without independent logical capture/reference calibration
remains inconclusive; never multiply dimensions or resample to manufacture evidence.

Paths are relative to the manifest. Export the original frame at scale 1 and
the drawn dimensions, without resizing it. Never create a reference from the
generated page, replace an original to pass a test, or raise thresholds to hide
wrong backgrounds, colors or assets. Pin the original hash and retain its Figma
file/node/version provenance alongside the manifest.

`verify-render.mjs` launches the runtime-audit Chromium runner and compares every
page screenshot with `devup-mcp-visual`. It writes the runtime report, screenshots,
`real-render.json` and `visual-diff-*.png`. Exit codes: 0 means runtime and pixels
passed; 1 means a runtime/strict-pixel failure or explicitly inconclusive comparison; 2 means tooling, input or reference
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
