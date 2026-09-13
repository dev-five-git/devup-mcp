# Keyframes screen: a static alignment difference

Investigated from unmodified `ee7fc6d616ecd0e3cd36ae63238d5ace91f44a0e`.

## Classification and measured baseline

The 6.71% discrepancy is **not a mid-flight animation capture**. The existing
render harness's `index.html` sets `animation: none !important` and
`transition: none !important` on every element and pseudo-element. At the
same readiness condition as capture, Chromium reports **zero animations**,
root `animation-name: none`, duration `0s`, transform `none`, and a 64×64
root at (0,0). No harness file was changed to establish this.

The worktree's own baseline executable has SHA-256
`c451093a2fb1bbeb436bf5a364055fa469fc45241476317a21e3e253fb22c352`.
Two fresh acquire/render runs reproduce keyframes **6.7138671875% (275/4096
pixels)** and grid **2.96%**, under their common theme `a1a8437993a0`.
These are measurements of the baseline, not copied threshold values.

`elements.mjs` finds no images or image backgrounds: the spinner consists
of CSS circles. The previous CROP-image defect is therefore inapplicable.
`bands.mjs` puts the worst eight-row band at y=48–56 (16.21%), and
`crop.mjs` shows horizontally displaced dots. `boxes.mjs` confirms their
12×12 wrappers have the correct positions and sizes.

## The collected chain and emitted keyframes

All eight chain frames are collected. They are sibling frames in a section,
with IDs `458:2021`, `458:2038`, `458:2055`, `458:2072`, `458:2089`,
`458:2106`, `458:2123`, `458:2140`, then back to `458:2021`. Each reaction
has `AFTER_TIMEOUT`, timeout `0.0010000000474974513`, a NODE action and
SMART_ANIMATE transition with duration `0.20000000298023224` and LINEAR easing.

The section-relative x coordinates are 89, 184, 268, 352, 436, 520, 604,
688; y is always 830. All direct, same-named child wrappers retain their
positions and 12×12 sizes. Their nested ellipse sizes vary. The documented
plugin rule compares direct children, finds no change, and falls back to
the frame itself; it does not recurse into those grandchildren.

The emitted animation exactly matches the stored plugin answer:

```tsx
animationDuration="1.6s"
animationFillMode="forwards"
animationIterationCount="infinite"
animationName={keyframes({
  "0%": { "transform": "translate(-95px, 0px)" },
  "13%": { "transform": "translate(95px, 0px)" },
  "25%": { "transform": "translate(84px, 0px)" },
  "100%": { "transform": "translate(-95px, 0px)" }
})}
animationTimingFunction="linear"
```

The first reverse difference is -95; successive frame deltas are +95 then
six +84 values. Repeated values are omitted by the plugin's incremental
keyframe rule. Eight transitions give 1.6s, first arrival rounds to 13%,
second to 25%, and the loop closes at 100%. The sub-10ms timeout emits no
delay. This explains byte parity; it does **not** claim that moving an entire
frame reproduces the prototype's changing nested dots. That inherited
animation limitation contributes zero pixels to this animation-disabled
capture and is separate from the measured static defect.

## Static cause and isolated browser probe

Each wrapper has `primaryAxisAlignItems: SPACE_BETWEEN`, horizontal layout,
and one visible in-flow ellipse. Figma's collected coordinates center the
single child on both axes. CSS `justify-content: space-between` places a
single flex item at the start of the main axis instead.

| Dot size | Wrapper position | Collected local dot position | Baseline global dot x | Correct global dot x |
| ---: | --- | --- | ---: | ---: |
| 2 | 44,8 | 5,5 | 44 | 49 |
| 4 | 52,26 | 4,4 | 52 | 56 |
| 6 | 44,44 | 3,3 | 44 | 47 |
| 8 | 26,52 | 2,2 | 26 | 28 |
| 9 | 8,44 | 1.5,1.5 | 8 | 9.5 |
| 10 | 0,26 | 1,1 | 0 | 1 |
| 11 | 8,7 | 0.5,0.5 | 8 | 8.5 |
| 12 | 26,0 | 0,0 | 26 | 26 |

A browser-only probe changes just those lone-item distributions to `center`.
The dots then occupy their collected positions, while root geometry and
animation state remain unchanged. With the same RGB(30,30,30) transparency
flattening and 24-channel tolerance, the discrepancy becomes **110/4096
pixels, 2.685546875%**: 165 fewer changed pixels, or **4.0283203125 percentage
points**. This is localisation evidence, not a measured generator candidate.
All 110 residual pixels lie within 1.02px of a designed circle circumference;
no pixel-tuned circle correction is proposed.

The layout cause is independent of the Smart Animate emission path. Scope
question `msg_559d8e972241` was sent to the coordinator before implementing a
correction; after its first ten-minute timeout, the same question was resumed
and escalation `msg_eab0d9b0b90b` identified the blocker. No permission was
inferred from elapsed time. No animation code, threshold, golden, or harness
script has been changed. The export already reports projection
`mapping-incomplete`: `DEVUP_CODEGEN_PROPERTY_UNMAPPED` explicitly names
`animationName` in the final JSX and delivery property audits. That existing
diagnostic is preserved, and its status is not relabeled as exact.

## Other fresh baseline screens

All 15 screens were freshly acquired with the preserved baseline executable
and rendered with the unchanged harness. The same binary bytes were retained
as `target/debug/devup-mcp-w19-baseline.exe` before test builds could replace
the ordinary executable. No input-identity check was bypassed.

| Group | Baseline divergence, narrow to wide |
| --- | --- |
| keyframes | 6.71% |
| grid | 2.96% |
| about | 5.51 / 3.01 / 1.77% |
| landing | 4.99 / 2.47 / 1.50% |
| notice | 5.25 / 3.22 / 2.20% |
| popup | 3.64 / 2.06 / 0.85% |
| report | 1.28% |

[keyframes-static-evidence.json](keyframes-static-evidence.json) preserves
the exact baseline metrics, binary identity, PNG hashes, and before/after
browser-probe DOM geometry. The probe's 2.69% remains separate from the
baseline measurements and is not used to lower any threshold.

## Verification and remaining work

The required gate ran in this worktree's own target with
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`,
`CARGO_INCREMENTAL=0`, and two jobs (`CARGO_BUILD_JOBS=2` for Insta).
`CARGO_TARGET_DIR` was never set.

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --locked --workspace --all-targets --all-features -j 2 -- -D warnings` | Pass, zero warnings |
| `cargo test --workspace -j 2 --no-fail-fast` | 1070 passed, 0 failed, 2 ignored |
| `cargo insta test --workspace --all-features --check` | 1070 passed, 0 failed, 2 ignored; no snapshots to review |
| `cargo test --locked -p devup-mcp --test stdio_smoke -j 2` | 2 passed, 0 failed |

Ordinary MSVC linking retains its existing localized library-creation
messages; the Clippy gate has no warnings. All 268 plugin goldens and their
manifest checks remain unchanged. No corpus assertion, Korean word-break
rule, forbidden Rust source, or harness script was modified.

**The requested generator improvement is not implemented.** This is not
category (b), so it would be incorrect to retire the 6.71% entry as a timing
artifact. There is a demonstrated static generator/layout cause outside the
original reaction-field constraint. Its correction requires the pending
scope expansion: test lone visible in-flow children first, reproduce RED,
emit the data-derived alignment, verify source mapping, reacquire with the
new binary, repeat keyframes/grid and all-screen regressions, then consider
lowering only measured thresholds. Hidden and absolutely positioned siblings,
vertical layout, and multi-item distributions need regression coverage.
No capture-specific constant, node ID, or breakpoint is needed.

Logs and probe artifacts are in the main checkout's ignored
`harness/render/out/w19-*` paths. The four preceding geometry reports were
read before investigation; no previously fixed text, stroke, crop, or mask
behavior is part of this cause.


## Authorized correction on top of b79ddbf (2026-09-13)

This continuation supersedes the historical pending-scope conclusion above;
`b79ddbf` itself is preserved. The coordinator explicitly authorized the general
lone-child layout correction, independent of reaction fields.

The baseline was rebuilt in this worktree's own target from the unchanged
production sources at `b79ddbf`. Its SHA-256 is
`989f7d27e265c80f7717d387458c11baea3440cf0ab23e9aa9bde5e6e8ae96c6`.
The build identity includes `-dirty` because the new RED test file existed when
the build script ran; no production source had changed. The executable was
preserved before candidate/test builds. Fresh acquisition and rendering reproduced
**275/4096 pixels, 6.7138671875%**, and grid **2.9639884816046393%**, twice.
The first all-screen run rejected popup tablet with
`ERR_INSUFFICIENT_RESOURCES`; that capture has no pixel result. The repeat was
valid at 2.0626068115234375%, and the other 14 screens reproduced exactly.

### RED, correction, and provenance

Before implementation, `cargo test --locked -p devup-mcp-devup-ui --test
lone_child_alignment -j 2` produced **2 passed / 2 failed**. The behavior failure
printed `<Flex ... justifyContent="space-between">`; the provenance failure was
`left: "raw-fallback", right: "derived-lone-child-center"`. The preservation
and non-flex checks passed. The RED log is
`harness/render/out/w19-fix-red.log` in this worktree.

`codegen/layout.rs` now emits `justifyContent="center"` only when the source
layout mode is horizontal or vertical, `primaryAxisAlignItems` is
`SPACE_BETWEEN`, and exactly one collected child is visible and not `ABSOLUTE`.
No node identity, viewport, animation field, dot size, or coordinate participates.
Zero- and multi-item distributions remain unchanged. The four integration tests
exercise both axes, hidden and absolutely positioned siblings, zero children,
multiple children, other distributions, grid/free layout, and the generated
source-map attribute. All four passed after implementation.

The `primaryAxisAlignItems` source-map entry uses
`derived-lone-child-center`; the public resolution glossary explains the child
count and explicitly disclaims pixel parity. It is a semantic conversion from
`SPACE_BETWEEN`, not an assertion that the source said `CENTER`.
The reaction emission is unchanged: root translation still matches the plugin's
`getReactionProps` answer and contributes no pixels with animations disabled.

### Other axes and distributions

CSS `space-between` falls back to start for one flex item, while `space-around`
already centers it. `space-evenly` also centers a lone item; the existing
mappings therefore need no correction. See the
[CSS Flexbox alignment rules](https://www.w3.org/TR/css-flexbox-1/#justify-content-property)
and [CSS Box Alignment distribution rules](https://www.w3.org/TR/css-align-3/#distribution-values).
Figma's [counterAxisAlignItems](https://developers.figma.com/docs/plugins/api/properties/nodes-counteraxisalignitems/)
supports MIN, MAX, CENTER and BASELINE, not a distribution mode.
Its separate [counterAxisAlignContent](https://developers.figma.com/docs/plugins/api/properties/nodes-counteraxisaligncontent/)
distributes wrapped tracks. This capture has NO_WRAP and centered counter-axis
items; it supplies no evidence for a wrapped-track correction. Neither path was
changed.


### Measured generator result and regression coverage

The candidate executable SHA-256 is
`bf28460b41f7b81ec66cb71e3f296a373fb9eba09c2a8cc7dcb67910bb2ea75f`.
All 15 candidate screens were freshly acquired with that preserved executable;
keyframes/grid and notice were reacquired again before the second all-screen
render. Both all-screen candidate runs were valid and their exact metrics
matched. No input hash check was bypassed. The baseline popup group received a
further valid repeat to complete the evidence for the rejected first capture.

| Screen group | Own baseline | Candidate, both runs |
| --- | --- | --- |
| keyframes | 6.7138671875% (275 pixels) | **2.685546875% (110 pixels)** |
| grid | 2.9639884816% | 2.9639884816% |
| about, narrow to wide | 5.51 / 3.01 / 1.77% | 5.51 / 3.01 / 1.77% |
| landing, narrow to wide | 4.99 / 2.47 / 1.50% | 4.99 / 2.47 / 1.50% |
| notice, narrow to wide | 5.25 / 3.22 / 2.2021854933% | 5.25 / 3.22 / **1.8804728546%** |
| popup, narrow to wide | 3.64 / 2.06 / 0.85% | 3.64 / 2.06 / 0.85% |
| report | 1.28% | 1.28% |

Keyframes improves by **165 pixels / 4.0283203125 percentage points**.
Notice desktop improves by **0.3217126386 percentage points** under the same
semantic correction: its 1920px header has one 1640px in-flow child at x=140 in
Figma. The mobile/tablet headers have no unused space after padding, so their
pixels do not change. All other exact divergence values remain identical.
Only the repeatedly measured keyframes and notice-desktop thresholds were
lowered, to **2.69** and **1.88**, respectively; tolerance is unchanged.

A later auxiliary subset capture rejected grid with
`ERR_INSUFFICIENT_RESOURCES`. It is retained as environment-invalid in the
evidence rather than counted as a measurement. A subsequent keyframes/grid
run was valid and reproduced 2.685546875% / 2.9639884816%. The main candidate
all-screen runs were both valid, including grid.

### Remaining 110 pixels: bounded negative

The generator now places every dot at the collected coordinates and dimensions.
All eight are opaque white full ellipses, with innerRadius=0 and no strokes or
effects in the collected data. Chromium reports zero animations. Every remaining
changed pixel lies within **1.0192024052px** of a designed circle circumference;
none lies farther inside or outside the circles. Per-dot counts for diameters
2, 4, 6, 8, 9, 10, 11 and 12 are **4, 0, 4, 8, 34, 6, 43 and 11**.

Two isolated browser probes preserve the measured layout geometry:

- Replacing CSS border-radius circles with equal-size SVG circles produces
  **pixel-identical output**, still 110 changed pixels against Figma.
- Adding `translateZ(0)` changes two low-delta pixel values but leaves the
  divergence and all per-dot counts unchanged at 110.

There is therefore no demonstrated second source-data or primitive-selection
correction to land. The next lead is **fractional paint-origin quantization and
edge coverage**, especially the 9px/11px dots whose boxes begin on half pixels;
together they account for 77 of the 110 pixels. Their DOM centers agree with
Figma, but intensity-weighted Chromium raster centroids are about half a pixel
right/down (the exact centroids and coverage sums are in the evidence JSON).
Even the unchanged 12px top dot has 11 edge differences, so fractional origins
alone cannot explain the whole remainder. A general rendering rule has not been
proven; no fitted translation, circle-size adjustment, or raster-specific
production workaround was added. There is no second fix commit.

### Final verification and contract review

| Gate, in this worktree's own target | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --locked --workspace --all-targets --all-features -j 2 -- -D warnings` | Pass, zero warnings |
| `cargo test --workspace -j 2 --no-fail-fast` | **1074 passed, 0 failed, 2 ignored** |
| `cargo insta test --workspace --all-features --check` | **1074 passed, 0 failed, 2 ignored**, no snapshots to review |
| `cargo test --locked -p devup-mcp --test stdio_smoke -j 2` | **2 passed, 0 failed** |

The first workspace pass found one expected snapshot addition: the public
resolution glossary in
`tests/snapshots/wquw_151__wquw_151_proofread_source_map.snap`. Its complete
content diff was reviewed and only that glossary entry accepted; no existing
mapping or assertion was removed. The subsequent full workspace and Insta
runs passed. This snapshot is outside `fixtures/devup-figma-plugin`.
All **268 plugin goldens and the manifest are unchanged**; the byte-parity and
corpus consistency tests pass. `korean_characters_keep_words_whole` passes.
No off-limits source or harness script was changed. Ordinary MSVC link steps
retain localized library-creation messages; Clippy reports zero warnings.

All Cargo invocations used `CARGO_PROFILE_DEV_DEBUG=0`,
`CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_INCREMENTAL=0`, and two jobs (including
`CARGO_BUILD_JOBS=2` for Insta). `CARGO_TARGET_DIR` was never set.

The `alignmentCorrection` section of
[keyframes-static-evidence.json](keyframes-static-evidence.json) retains exact
before/after metrics, binary identities, both invalid captures and their valid
retries, PNG hashes, notice-node geometry, and the residual probes. Build and
test logs are this worktree's ignored `harness/render/out/w19-fix-*` files;
acquire/render and probe logs are the main checkout's ignored paths of the same
name. The implementation is committed on this branch without pushing; the own
target is cleaned after committing and task-started servers are stopped.
