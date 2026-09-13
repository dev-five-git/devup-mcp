# Grid: correct cell placement, lossy raster composition

Investigated from clean `937313e39448db7f7c05dbb0ab206432261b2203` in
`W24-C-grid`. This is a quantified negative: no production correction is
committed and no threshold is lowered. The discrepancy is localised to three
exported photographs, not the CSS grid track projection. Two distinct raster
effects remain: shrinking integer PNG canvases into fractional layout boxes,
and the sampling difference between individual-node and whole-frame exports.

## Own baseline and localisation before Rust

The own-target baseline executable was preserved before test builds as
`harness/render/out/w24-grid-baseline.exe`, SHA-256
`27ca7e30f7fea3f1e76fe6e5049e46d1b5461d2b461a7bf8d7e0fef864c831c7`.
It reports build `937313e39448`, without a dirty suffix. The isolated harness
uses unchanged tracked scripts, copied call-bank inputs, its own generated
files, and the existing unchanged visual comparator from main. No acquisition
identity check was bypassed. Both screens were reacquired and rendered twice.

| Screen | Baseline run 1 | Baseline run 2 | Rendered size |
| --- | ---: | ---: | --- |
| grid-429-1966 | 41,234 / 1,391,166 = 2.9639884816% | identical | 1281x1086 |
| keyframes-458-2021 | 110 / 4,096 = 2.6855468750% | identical | 64x64 |

Both use theme SHA-256
`a1a8437993a02c99489dfdc71ca7e2f5d6447f4956600fd939272816bc0f3f63`.
The before numbers are measured, not taken from thresholds. Full metrics,
input hashes, node fields and repeated probes are in
[grid-raster-evidence.json](grid-raster-evidence.json).

`bands.mjs`, `drift.mjs`, `crop.mjs`, `boxes.mjs` and `elements.mjs` were used.
The first 300 rows contain zero differing pixels; y=400–500 contains 9,290,
and y=1000–1086 contains 7,699. Most bands prefer zero vertical offset; the
last band prefers -1, consistent with internal rescaling rather than page
height accumulation. Full-resolution crops show photographic detail changes.

Every differing pixel belongs to one of the last three asset boxes. The
following partition assigns each pixel once, including fractional edge pixels.
No differing pixel remains in gaps or outside the photographs.

| Node | Source local x,y | Source width,height | PNG width,height | Baseline changed pixels |
| --- | --- | --- | --- | ---: |
| 429:1967 | 0,0 | 847,348 | 847,348 | 0 |
| 429:1968 | 867,0 | 413,348 | 413,348 | 0 |
| 429:1969 | 0,368 | 413,717 | 413,717 | 0 |
| 429:1970 | 433.3333129882813,368 | 847,349 | 847,349 | 24,299 |
| 429:1971 | 433.3333129882813,737 | 413.3333435058594,348.3333435058594 | 414,349 | 10,488 |
| 429:1972 | 867,737 | 413.3333435058594,348.3333435058594 | 414,349 | 6,447 |

## What is ruled out

Only after measurement and localisation was `grid_template` inspected. Its
three HUG tracks emit `repeat(3, fit-content(100%))` on both axes. Chromium
resolves columns to 413.328125, 413.671875 and 413.5px and rows to 348,349,
348.328125px. Image origins are (0,0), (867,0), (0,368), (433.328125,368),
(433.328125,737), (867,737). These match the collected origins to CSS layout
precision. The root's screenshot size is the integer canvas; its source box
is actually 1280.3333740234375x1085.3333740234375.

An isolated browser probe removes grid placement from the equation by
absolutely positioning every image at its **collected** x,y. It reproduces all
41,234 changed pixels and every per-image count exactly. This probe collapses
the empty grid's intrinsic height, so it is diagnostic only, never a candidate
layout. Restoring full source precision for image dimensions also reproduces
all counts. Rounding origins makes the result worse, at 47,524 pixels. There
is no evidence to replace HUG tracks or emit fitted grid dimensions.

All seven prior reports were read. This snapshot has seven nodes: one grid
frame and six leaf assets. There is no live text, no painted stroke array,
no mask, no nested live child composition, no CROP fill, no effects and no
animation. The six image fills have FILL mode, identity image transforms,
opacity 1 and zero image filters. Therefore the previously fixed text, weight,
whitespace, overflow, mask, crop, lone-flex-child and outside-edge mechanisms
do not explain these pixels. The intentional Korean allowance is **0pp here**:
there are no generated text nodes. No Korean policy or test was changed.

## Raster probes and what resisted correction

Probes start from the unchanged generated page. PNGs are flattened onto the
same RGB(30,30,30) canvas and compared at channel tolerance 24. Every final
paired probe also captures keyframes, which remains at 110 pixels throughout;
both paired suites repeat identically. Early exploratory grid-only runs are
not substituted for the paired confirmation.

| Browser intervention | Grid pixels | 429:1970 / 1971 / 1972 | Keyframes pixels |
| --- | ---: | --- | ---: |
| Baseline | 41,234 | 24,299 / 10,488 / 6,447 | 110 |
| Full-precision source sizes | 41,234 | 24,299 / 10,488 / 6,447 | 110 |
| Collected source positions | 41,234 | 24,299 / 10,488 / 6,447 | 110 |
| Rounded source positions | 47,524 | 24,299 / 16,778 / 6,447 | 110 |
| object-fit:none; object-position:left top | 29,333 | 24,299 / 5,028 / 6 | 110 |
| Natural paint plus source positions | 29,333 | 24,299 / 5,028 / 6 | 110 |
| translateZ(0) | 41,234 | 24,299 / 10,488 / 6,447 | 110 |
| Natural paint plus translateZ(0) | 29,333 | 24,299 / 5,028 / 6 | 110 |
| SVG image primitive at source boxes | 39,806 | 24,299 / 12,236 / 3,271 | 110 |
| SVG image at natural paint dimensions | 29,333 | 24,299 / 5,028 / 6 | 110 |

The natural-paint probe removes 11,901 pixels, **0.855469% of frame area**,
without changing any layout box. This is a browser-only delta, not a shipped
generator improvement. The bottom-right image independently confirms the
mechanism: its unscaled exported PNG at its integer source origin differs
from the whole-frame reference in only four pixels (zero in the two-pixel
inset interior), whereas shrinking it into the fractional box produces 6,447.

Natural painting does not solve the largest image. Node 429:1970 has an
integer 847x349 source/export size; its standalone PNG itself already differs
from the reference region by 24,299 pixels. There is no double size conversion
to remove there. Its fractional source x is the distinguishing field. A
numeric bilinear sampling probe on the two-pixel inset interior reduces
23,693 differences to 12,230 at +1/3px horizontal phase and zero vertical
phase, but does not recover the original whole-frame samples. The analogous
429:1971 interior falls from 4,366 to 1,552. These are bounded diagnostic
sampling tests, not permission to fit a translation or blur to the capture.
No subpixel offset, width branch, node special case or filter is introduced.

`assets.js` verifies the requested image fill, then calls `node.exportAsync`
with a SCALE constraint; it does not supply the raw photograph. The export
API supports density scales 1–4. `CodegenOptions` and `push_object_fit` do not
receive the selected export scale or the actual PNG canvas dimensions.
Blindly emitting `objectFit="none"` for fractional nodes would paint a 2x
export at twice its intended size. Inferring a 1x export from a fractional
layout width is invalid. Existing `export_offset` compares geometric render
bounds with layout bounds; in this capture they coincide, so it cannot supply
the missing raster canvas/density semantics either.

A separate request asked for scale-2 exports of the three affected assets,
writing new filenames rather than replacing acquired inputs. It returned no
response or files after more than five minutes and was stopped; its own
Python/server processes were terminated. Thus no scale-2 measurement is
claimed. The scale concern above follows the supported API and CSS intrinsic
sizing, not a fabricated scale-2 screenshot result.

The next correction needs a density-aware mapping from exported pixel canvas
to logical paint bounds, with scale-1 and scale-2 evidence and truthful source
mapping. Fractional-origin sampling additionally needs validation against the
whole-frame reference; recomposing already rasterised samples cannot be assumed
to recover it. A raw-image or vector export strategy would need its own crop,
clipping and density proof. This is the specific representation boundary left
open, rather than an uninvestigated CSS-grid lead.

## Delivery and verification

No Rust, plugin golden, manifest checksum, plugin build, threshold, provenance,
fidelity or source-map output is changed. There is no production candidate and
therefore no synthetic failing-test claim: the work remains a measured negative,
as permitted by the task. All 268 plugin goldens retain their existing bytes.
No off-limits file or tracked harness script is edited. Shared layout/style/text
and provenance files have **no overlap** for the coordinator to merge.

The final fresh acquire/render again produced 41,234 grid pixels and 110
keyframes pixels, with both actual PNGs byte-identical to the first baseline.
It used the preserved baseline executable; no test rebuild was silently
substituted for the acquired binary.

| Required own-target gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --locked --workspace --all-targets --all-features -j 2 -- -D warnings` | Pass, zero warnings |
| `cargo test --workspace -j 2 --no-fail-fast` | 1,112 passed / 0 failed / 2 ignored |
| `cargo insta test --workspace --all-features --check` | 1,112 passed / 0 failed / 2 ignored; no snapshots to review |
| `cargo test --locked -p devup-mcp --test stdio_smoke -j 2` | 2 passed / 0 failed |

The unchanged corpus consistency test and Korean preservation test pass.
Ordinary MSVC linking emits the pre-existing library-creation messages;
Clippy itself has zero warnings. Gate logs are
`harness/render/out/w24-{fmt,clippy,workspace,insta,smoke}.log`.
All Cargo invocations use two jobs, `CARGO_PROFILE_DEV_DEBUG=0`,
`CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_INCREMENTAL=0`; `CARGO_TARGET_DIR` is never
set. Raw logs and probe scripts are under ignored `harness/render/out/w24-*`
and `harness/render/out/grid-*`. Only this report and its evidence JSON are
included in the local commit; nothing is pushed. Delivery cleanup runs
`cargo clean` after the commit and checks that no task-started generator or
preview server remains before sending the worker outcome.
